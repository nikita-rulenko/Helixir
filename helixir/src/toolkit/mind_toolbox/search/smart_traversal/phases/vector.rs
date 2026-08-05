//! Vector and BM25 seed retrieval phase.

use super::bm25::fetch_bm25_memories;
use super::*;

pub async fn vector_search_phase(
    client: Arc<HelixClient>,
    query_text: &str,
    query_embedding: &[f32],
    user_id: Option<&str>,
    config: &SearchConfig,
    window: &TimeWindow,
    profile: RetrievalProfile,
) -> Result<Vec<SearchResult>, TraversalError> {
    let top_k = config.vector_top_k;
    let min_score = config.min_vector_score;
    info!("Starting Phase 1: Vector search with top_k={}", top_k);

    let fetch_limit = if user_id.is_some() {
        top_k as i64 * 3
    } else {
        top_k as i64
    };
    let query_vector: Vec<f64> = query_embedding.iter().map(|&x| x as f64).collect();

    // #31 bi-temporality: the window filters on EVENT time (valid_from else
    // created_at) — HQL can't express the coalesce, so the cutoff pushdown
    // (smartVectorSearchWithChunksCutoff, still in queries.hx) is not used
    // and the Rust-side filter below is authoritative. Explicit windows are
    // the rare path; the overfetch already covers the delta.
    let vector_response: VectorSearchResponse = {
        let params = serde_json::json!({
            "query_vector": query_vector,
            "limit": fetch_limit
        });
        client
            .execute_query("smartVectorSearchWithChunks", &params)
            .await
            .map_err(|e| TraversalError::Database(e.to_string()))?
    };

    let bm25_limit = fetch_limit.saturating_mul(2).max(fetch_limit);
    let bm25_memories: Option<Vec<VectorMemory>> = if profile.native_hybrid_bm25() {
        match fetch_bm25_memories(&client, query_text, bm25_limit).await {
            Ok(rows) if !rows.is_empty() => Some(rows),
            Ok(_) => {
                debug!("BM25 returned no rows; using vector ordering only");
                None
            }
            Err(e) => {
                warn!(
                    "BM25 hybrid skipped (is bm25=true in Helix and query deployed?): {}",
                    e
                );
                None
            }
        }
    } else {
        None
    };

    let visit_order: Vec<String> = if let Some(ref bm25_rows) = bm25_memories {
        let v_ids: Vec<String> = vector_response
            .memories
            .iter()
            .filter(|m| !m.memory_id.is_empty())
            .map(|m| m.memory_id.clone())
            .collect();
        let b_ids: Vec<String> = bm25_rows
            .iter()
            .filter(|m| !m.memory_id.is_empty())
            .map(|m| m.memory_id.clone())
            .collect();
        info!(
            "Phase 1 hybrid (RRF k=60): merging {} vector + {} BM25 hits",
            v_ids.len(),
            b_ids.len()
        );
        rrf::fused_memory_order(&v_ids, &b_ids)
    } else {
        vector_response
            .memories
            .iter()
            .filter(|m| !m.memory_id.is_empty())
            .map(|m| m.memory_id.clone())
            .collect()
    };

    let mut memory_by_id: HashMap<String, VectorMemory> = HashMap::new();
    for m in &vector_response.memories {
        if m.memory_id.is_empty() {
            continue;
        }
        memory_by_id
            .entry(m.memory_id.clone())
            .or_insert_with(|| m.clone());
    }
    if let Some(rows) = bm25_memories {
        for m in rows {
            if m.memory_id.is_empty() {
                continue;
            }
            memory_by_id.entry(m.memory_id.clone()).or_insert(m);
        }
    }

    let mut results = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut accepted_rank: usize = 0;

    for memory_id in visit_order {
        let Some(memory) = memory_by_id.get(&memory_id) else {
            continue;
        };
        if let Some(uid) = user_id {
            if memory.user_id.is_empty() {
                warn!(
                    "Memory {} has empty user_id, including in results for verification",
                    memory.memory_id
                );
            } else if memory.user_id != uid {
                continue;
            }
        }

        if seen_ids.contains(&memory.memory_id) {
            continue;
        }
        seen_ids.insert(memory.memory_id.clone());

        // #87: the window hard-filters SEEDS only — expansion is exempt and
        // out-of-window rows return as flagged flashbacks instead. Rust-side
        // filter is authoritative for both arms (BM25 rows arrive unfiltered
        // from HQL anyway — P0.1 defence in depth).
        if window.is_active() {
            let when = super::super::scoring::event_time(&memory.valid_from, &memory.created_at);
            if !window.contains_rfc3339(when) {
                continue;
            }
        }

        let vector_score = config.rank_base * config.rank_decay.powi(accepted_rank as i32);
        accepted_rank += 1;

        let temporal_score = calculate_temporal_freshness(
            super::super::scoring::event_time(&memory.valid_from, &memory.created_at),
            config.temporal_decay_days,
        );

        let mut result = SearchResult::from_vector_weighted(
            &memory.memory_id,
            &memory.content,
            vector_score,
            temporal_score,
            config.vector_weight,
            config.temporal_weight,
        );
        result.internal_id = (!memory.id.is_empty()).then(|| memory.id.clone());
        result.created_at = Some(memory.created_at.clone());
        result.valid_from = Some(memory.valid_from.clone());

        let mut meta = HashMap::new();
        if !memory.user_id.is_empty() {
            meta.insert(
                "user_id".to_string(),
                serde_json::Value::String(memory.user_id.clone()),
            );
        }
        if !memory.memory_type.is_empty() {
            meta.insert(
                "memory_type".to_string(),
                serde_json::Value::String(memory.memory_type.clone()),
            );
        }
        if !memory.content_key.is_empty() {
            meta.insert(
                "content_key".to_string(),
                serde_json::Value::String(memory.content_key.clone()),
            );
        }
        if profile.native_hybrid_bm25() {
            meta.insert(
                "phase1_hybrid".to_string(),
                serde_json::Value::String("vector_rrf_bm25".to_string()),
            );
        }
        if profile.result_provenance() {
            meta.insert(
                "origin".to_string(),
                serde_json::Value::String("seed".to_string()),
            );
        }
        if !meta.is_empty() {
            result.metadata = Some(meta);
        }

        if result.combined_score >= min_score {
            results.push(result);
        }
    }

    results.sort_by(|a, b| {
        crate::toolkit::mind_toolbox::ranking::desc(&a.combined_score, &b.combined_score)
    });

    if !results.is_empty() {
        let top = results.first().unwrap().combined_score;
        let bottom = results.last().unwrap().combined_score;
        info!(
            "Phase 1 completed: {} results, score range {:.4}..{:.4} (spread {:.4})",
            results.len(),
            top,
            bottom,
            top - bottom
        );
    } else {
        info!("Phase 1 completed: 0 results");
    }
    Ok(results)
}
