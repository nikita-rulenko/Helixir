use super::models::{GraphScores, ScoreWeights, SearchConfig, SearchResult};
use super::rrf;
use super::scoring::{calculate_graph_score, calculate_temporal_freshness};
use crate::core::{RetrievalProfile, TimeWindow};
use crate::db::HelixClient;
use crate::utils::nullable_string;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{debug, info, warn};

#[derive(Debug, thiserror::Error)]
pub enum TraversalError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Invalid query: {0}")]
    InvalidQuery(String),
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)] // `chunks` mirrors the HelixDB response shape; kept for parity / future use.
struct VectorSearchResponse {
    #[serde(default)]
    memories: Vec<VectorMemory>,
    #[serde(default)]
    chunks: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize, Clone)]
struct VectorMemory {
    #[serde(default, deserialize_with = "nullable_string")]
    memory_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    content: String,
    #[serde(default, deserialize_with = "nullable_string")]
    created_at: String,
    #[serde(default, deserialize_with = "nullable_string")]
    valid_from: String,
    #[serde(default, deserialize_with = "nullable_string")]
    memory_type: String,
    #[serde(default, deserialize_with = "nullable_string")]
    user_id: String,
}

#[derive(Debug, Deserialize, Default)]
struct GraphConnectionsResponse {
    #[serde(default)]
    implies_out: Vec<ConnectedMemory>,
    #[serde(default)]
    implies_in: Vec<ConnectedMemory>,
    #[serde(default)]
    because_out: Vec<ConnectedMemory>,
    #[serde(default)]
    because_in: Vec<ConnectedMemory>,
    #[serde(default)]
    contradicts_out: Vec<ConnectedMemory>,
    #[serde(default)]
    contradicts_in: Vec<ConnectedMemory>,
    #[serde(default)]
    relation_out: Vec<ConnectedMemory>,
    #[serde(default)]
    relation_in: Vec<ConnectedMemory>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // `memory_type` reflected from HelixDB; reserved for upcoming filters.
struct ConnectedMemory {
    #[serde(default, deserialize_with = "nullable_string")]
    memory_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    content: String,
    #[serde(default, deserialize_with = "nullable_string")]
    created_at: String,
    #[serde(default, deserialize_with = "nullable_string")]
    valid_from: String,
    #[serde(default, deserialize_with = "nullable_string")]
    memory_type: String,
}

async fn fetch_bm25_memories(
    client: &HelixClient,
    query_text: &str,
    limit: i64,
) -> Result<Vec<VectorMemory>, TraversalError> {
    #[derive(Debug, Deserialize)]
    struct Bm25Response {
        #[serde(default)]
        memories: Vec<VectorMemory>,
    }

    let params = serde_json::json!({
        "text": query_text,
        "limit": limit,
    });

    let resp: Bm25Response = client
        .execute_query("searchMemoriesByBm25", &params)
        .await
        .map_err(|e| TraversalError::Database(e.to_string()))?;
    Ok(resp.memories)
}

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
            let when = super::scoring::event_time(&memory.valid_from, &memory.created_at);
            if !window.contains_rfc3339(when) {
                continue;
            }
        }

        let vector_score = config.rank_base * config.rank_decay.powi(accepted_rank as i32);
        accepted_rank += 1;

        let temporal_score = calculate_temporal_freshness(
            super::scoring::event_time(&memory.valid_from, &memory.created_at),
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

pub async fn graph_expansion_phase(
    client: Arc<HelixClient>,
    vector_hits: &[SearchResult],
    config: &SearchConfig,
) -> Result<Vec<SearchResult>, TraversalError> {
    info!(
        "Starting Phase 2: Graph expansion from {} vector hits",
        vector_hits.len()
    );

    let mut all_results = Vec::new();
    let mut expansion_tasks = Vec::new();

    let max_depth = config.graph_depth;
    let graph_weights = (
        config.graph_semantic_weight,
        config.graph_graph_weight,
        config.graph_temporal_weight,
        config.temporal_decay_days,
    );
    let ew = config.edge_weights;
    let ed = config.edge_damping;

    for hit in vector_hits {
        let ctx = ExpandCtx {
            client: Arc::clone(&client),
            max_depth,
            graph_weights,
            ew,
            ed,
        };
        let hit = hit.clone();

        let task = tokio::spawn(async move {
            let mut visited = HashSet::new();
            visited.insert(hit.memory_id.clone());

            expand_from_node(&ctx, &hit.memory_id, 1, &mut visited, hit.combined_score).await
        });

        expansion_tasks.push(task);
    }

    for task in expansion_tasks {
        match task.await {
            Ok(Ok(results)) => all_results.extend(results),
            Ok(Err(e)) => warn!("Graph expansion failed: {}", e),
            Err(e) => warn!("Graph expansion task panicked: {}", e),
        }
    }

    info!("Phase 2 completed: {} expanded results", all_results.len());
    Ok(all_results)
}

/// Read-only invariants of one graph expansion (#9): everything that stays
/// constant while `expand_from_node` recurses.
struct ExpandCtx {
    client: Arc<HelixClient>,
    max_depth: u32,
    /// (semantic_w, graph_w, temporal_w, decay_days)
    graph_weights: (f64, f64, f64, f64),
    ew: crate::core::config::EdgeWeights,
    ed: crate::core::config::EdgeDamping,
}

async fn expand_from_node(
    ctx: &ExpandCtx,
    node_id: &str,
    current_depth: u32,
    visited: &mut HashSet<String>,
    parent_score: f64,
) -> Result<Vec<SearchResult>, TraversalError> {
    let ExpandCtx { ew, ed, .. } = ctx;
    debug!("Expanding from node {} at depth {}", node_id, current_depth);

    let params = serde_json::json!({
        "memory_id": node_id
    });

    let response: GraphConnectionsResponse = ctx
        .client
        .execute_query("getMemoryLogicalConnections", &params)
        .await
        .map_err(|e| TraversalError::Database(e.to_string()))?;

    let mut results = Vec::new();
    let mut neighbors = Vec::new();

    process_edge_collection(
        ctx,
        &response.implies_out,
        ("IMPLIES", ew.implies),
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        ctx,
        &response.because_out,
        ("BECAUSE", ew.because),
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        ctx,
        &response.contradicts_out,
        ("CONTRADICTS", ew.contradicts),
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        ctx,
        &response.relation_out,
        ("MEMORY_RELATION", ew.memory_relation),
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        ctx,
        &response.implies_in,
        ("IMPLIES_IN", ew.implies * ed.implies_in),
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        ctx,
        &response.because_in,
        ("BECAUSE_IN", ew.because * ed.because_in),
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        ctx,
        &response.contradicts_in,
        ("CONTRADICTS_IN", ew.contradicts * ed.contradicts_in),
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        ctx,
        &response.relation_in,
        ("MEMORY_RELATION_IN", ew.memory_relation * ed.relation_in),
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    if current_depth < ctx.max_depth {
        neighbors.sort_by(|a, b| crate::toolkit::mind_toolbox::ranking::desc(&a.1, &b.1));
        for (neighbor_id, neighbor_score) in neighbors.into_iter().take(3) {
            if !visited.contains(&neighbor_id) {
                visited.insert(neighbor_id.clone());
                let expanded = Box::pin(expand_from_node(
                    ctx,
                    &neighbor_id,
                    current_depth + 1,
                    visited,
                    neighbor_score,
                ))
                .await?;
                results.extend(expanded);
            }
        }
    }

    Ok(results)
}

fn process_edge_collection(
    ctx: &ExpandCtx,
    memories: &[ConnectedMemory],
    edge: (&str, f64),
    parent_score: f64,
    visited: &HashSet<String>,
    results: &mut Vec<SearchResult>,
    neighbors: &mut Vec<(String, f64)>,
) {
    let (edge_type, edge_weight) = edge;
    let (semantic_w, graph_w, temporal_w, decay_days) = ctx.graph_weights;

    for mem in memories {
        if visited.contains(&mem.memory_id) {
            continue;
        }

        let temporal_score = calculate_temporal_freshness(
            super::scoring::event_time(&mem.valid_from, &mem.created_at),
            decay_days,
        );
        let graph_score = calculate_graph_score(edge_weight, parent_score);

        let semantic_sim = 0.5;

        let mut result = SearchResult::from_graph_weighted(
            &mem.memory_id,
            &mem.content,
            GraphScores {
                semantic_sim,
                graph_score,
                temporal_score,
            },
            1,
            vec![edge_type.to_string()],
            ScoreWeights {
                semantic: semantic_w,
                graph: graph_w,
                temporal: temporal_w,
            },
        );
        result.created_at = Some(mem.created_at.clone());
        result.valid_from = Some(mem.valid_from.clone());

        results.push(result);
        neighbors.push((mem.memory_id.clone(), graph_score));
    }
}

pub fn rank_and_filter(results: Vec<SearchResult>, min_combined_score: f64) -> Vec<SearchResult> {
    info!(
        "Starting Phase 3: Ranking and filtering {} results",
        results.len()
    );

    let mut best_scores: std::collections::HashMap<String, SearchResult> =
        std::collections::HashMap::new();

    for result in results {
        match best_scores.get(&result.memory_id) {
            Some(existing) => {
                if result.combined_score > existing.combined_score {
                    best_scores.insert(result.memory_id.clone(), result);
                }
            }
            None => {
                best_scores.insert(result.memory_id.clone(), result);
            }
        }
    }

    let mut filtered_results: Vec<SearchResult> = best_scores
        .into_values()
        .filter(|r| r.combined_score >= min_combined_score)
        .collect();

    filtered_results.sort_by(|a, b| {
        crate::toolkit::mind_toolbox::ranking::desc(&a.combined_score, &b.combined_score)
    });

    info!(
        "Phase 3 completed: {} final results",
        filtered_results.len()
    );
    filtered_results
}
