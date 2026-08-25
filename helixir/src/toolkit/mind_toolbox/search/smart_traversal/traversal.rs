use super::batch_expansion::graph_expansion_phase_batched;
use super::models::{SearchConfig, SearchResult, TraversalStats};
use super::phases::{TraversalError, graph_expansion_phase, rank_and_filter, vector_search_phase};
use super::ppr::personalized_pagerank;
use super::scoring::{
    calculate_graph_combined_score_weighted, calculate_vector_combined_score_weighted, cosine_score,
};
use crate::core::{RetrievalProfile, TimeWindow};
use crate::db::HelixClient;
use crate::llm::EmbeddingGenerator;
use lru::LruCache;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

#[derive(Clone)]
struct CacheEntry {
    results: Vec<SearchResult>,
    inserted_at: Instant,
}

pub struct SmartTraversalV2 {
    client: Arc<HelixClient>,
    embedder: Arc<EmbeddingGenerator>,
    cache: RwLock<LruCache<String, CacheEntry>>,
    cache_ttl: Duration,
    profile: RetrievalProfile,
    stats: RwLock<TraversalStats>,
}

impl SmartTraversalV2 {
    pub fn new(
        client: Arc<HelixClient>,
        embedder: Arc<EmbeddingGenerator>,
        cache_size: usize,
        cache_ttl_secs: u64,
    ) -> Self {
        Self::with_profile(
            client,
            embedder,
            cache_size,
            cache_ttl_secs,
            RetrievalProfile::from_env(),
        )
    }

    pub fn with_profile(
        client: Arc<HelixClient>,
        embedder: Arc<EmbeddingGenerator>,
        cache_size: usize,
        cache_ttl_secs: u64,
        profile: RetrievalProfile,
    ) -> Self {
        Self {
            client,
            embedder,
            cache: RwLock::new(LruCache::new(
                std::num::NonZeroUsize::new(cache_size).unwrap(),
            )),
            cache_ttl: Duration::from_secs(cache_ttl_secs),
            profile,
            stats: RwLock::new(TraversalStats::default()),
        }
    }

    pub async fn search(
        &self,
        query: &str,
        query_embedding: &[f32],
        user_id: Option<&str>,
        config: SearchConfig,
        window: TimeWindow,
    ) -> Result<Vec<SearchResult>, TraversalError> {
        let cache_key = self.make_cache_key(query, query_embedding, user_id, &config, &window);

        {
            let mut cache = self.cache.write().await;
            if let Some(entry) = cache.get(&cache_key) {
                let ttl_ok = !self.profile.cache_correctness_fixes()
                    || entry.inserted_at.elapsed() < self.cache_ttl;
                if ttl_ok {
                    let cached_results = entry.results.clone();
                    let mut stats = self.stats.write().await;
                    stats.cache_hits += 1;
                    stats.cache_hit_rate =
                        stats.cache_hits as f64 / (stats.cache_hits + stats.cache_misses) as f64;
                    debug!("Cache hit for query: {}", query);
                    return Ok(cached_results);
                } else {
                    debug!(
                        "Cache entry expired (ttl={}s) for query: {}",
                        self.cache_ttl.as_secs(),
                        query
                    );
                    cache.pop(&cache_key);
                }
            }
        }

        let start_time = Instant::now();
        info!(
            "Starting smart traversal search for query: {} (profile={})",
            query,
            self.profile.tag()
        );

        {
            let mut stats = self.stats.write().await;
            stats.cache_misses += 1;
            stats.cache_hit_rate =
                stats.cache_hits as f64 / (stats.cache_hits + stats.cache_misses) as f64;
        }

        let phase1_start = Instant::now();
        let mut vector_hits = vector_search_phase(
            Arc::clone(&self.client),
            query,
            query_embedding,
            user_id,
            &config,
            &window,
            self.profile,
        )
        .await?;
        let phase1_duration = phase1_start.elapsed();

        if vector_hits.is_empty() {
            info!("No vector hits found, returning empty results");
            let total_duration = start_time.elapsed();
            let mut stats = self.stats.write().await;
            stats.phase1_duration_ms = phase1_duration.as_millis() as f64;
            stats.total_duration_ms = total_duration.as_millis() as f64;
            return Ok(vec![]);
        }

        let rerank_start = Instant::now();
        let texts: Vec<&str> = vector_hits.iter().map(|h| h.content.as_str()).collect();
        match self.embedder.generate_batch(&texts, true).await {
            Ok(embeddings) => {
                let mut reranked = 0u32;
                for (hit, emb) in vector_hits.iter_mut().zip(embeddings.iter()) {
                    let real_score = cosine_score(query_embedding, emb);
                    if rerank_seed(hit, real_score, &config) {
                        reranked += 1;
                    }
                }
                vector_hits.sort_by(|a, b| {
                    crate::toolkit::mind_toolbox::ranking::desc(
                        &a.combined_score,
                        &b.combined_score,
                    )
                });
                let rerank_ms = rerank_start.elapsed().as_millis();
                if reranked > 0 {
                    let top = vector_hits.first().unwrap().combined_score;
                    let bot = vector_hits.last().unwrap().combined_score;
                    info!(
                        "Re-ranked {}/{} results with real cosine similarity in {}ms, scores {:.4}..{:.4}",
                        reranked,
                        vector_hits.len(),
                        rerank_ms,
                        top,
                        bot
                    );
                }
            }
            Err(e) => {
                warn!("Re-ranking failed (using rank-based scores): {}", e);
            }
        }

        let phase2_start = Instant::now();
        let (mut graph_results, ego_edges) = if self.profile.batched_graph_expansion() {
            let expansion =
                graph_expansion_phase_batched(Arc::clone(&self.client), &vector_hits, &config)
                    .await?;
            (expansion.results, expansion.edges)
        } else {
            let results =
                graph_expansion_phase(Arc::clone(&self.client), &vector_hits, &config).await?;
            (results, Vec::new())
        };
        let phase2_duration = phase2_start.elapsed();

        if self.profile.real_cosine_for_graph_nodes() && !graph_results.is_empty() {
            self.rerank_graph_results(query_embedding, &mut graph_results, &config)
                .await;
        }

        // Elder-brain #9: blend PPR mass over the typed ego-network into the
        // final rank of every result (seeds included), replacing the per-hop
        // multiplicative decay that buried distant-but-coherent nodes.
        if self.profile.ppr_ranking() && !ego_edges.is_empty() {
            let personalization: std::collections::HashMap<String, f64> = vector_hits
                .iter()
                .map(|h| (h.memory_id.clone(), h.combined_score.max(0.01)))
                .collect();
            let ppr_scores = personalized_pagerank(
                &ego_edges,
                &personalization,
                config.ppr_alpha,
                config.ppr_iterations,
            );
            let mut rescored = 0usize;
            for result in vector_hits.iter_mut().chain(graph_results.iter_mut()) {
                let Some(ppr) = ppr_scores.get(&result.memory_id) else {
                    continue;
                };
                let direct_retrieval_score = result.combined_score;
                let has_bm25_evidence = result
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("bm25_rank"))
                    .is_some();
                let hybrid_semantic_floor = if has_bm25_evidence {
                    result.vector_score
                } else {
                    0.0
                };
                result.graph_score = *ppr;
                let graph_blend = (config.graph_semantic_weight * result.vector_score
                    + config.graph_graph_weight * ppr
                    + config.graph_temporal_weight * result.temporal_score)
                    .clamp(0.0, 1.0);
                // PPR is additional graph evidence, not permission to erase a
                // direct retrieval hit. In particular, a BM25-backed fact may
                // be old and graph-isolated: replacing its hybrid semantic
                // score with the graph blend made exact lexical matches
                // disappear from the honest top-K. The floor is the already
                // blended cosine/BM25 score, not the raw BM25 rank score, so a
                // weak lexical rank-1 does not receive an artificial 0.95.
                // Expanded rows have no independent retrieval evidence, so
                // only seeds receive this floor.
                result.combined_score = preserve_direct_seed_score(
                    &result.source,
                    direct_retrieval_score,
                    hybrid_semantic_floor,
                    graph_blend,
                );
                if let Some(meta) = result.metadata.as_mut() {
                    meta.insert(
                        "ppr".to_string(),
                        serde_json::Value::from((ppr * 1000.0).round() / 1000.0),
                    );
                    // Raw cosine survives next to the blended score: the
                    // write-path duplicate gate needs the pure semantic
                    // signal, not the rank blend (#32 W2).
                    meta.entry("cosine".to_string()).or_insert_with(|| {
                        serde_json::Value::from((result.vector_score * 1000.0).round() / 1000.0)
                    });
                    meta.insert(
                        "semantic_score".to_string(),
                        serde_json::Value::from((result.vector_score * 1000.0).round() / 1000.0),
                    );
                }
                rescored += 1;
            }
            info!(
                "PPR re-rank: {} results rescored over {} ego edges",
                rescored,
                ego_edges.len()
            );
        }

        // #87: graph expansion is EXEMPT from the window — rows it pulled
        // from outside come back as flagged flashbacks (with their event
        // date) instead of being hidden. Seeds are in-window by construction.
        if window.is_active() {
            let mut flashbacks = 0usize;
            for result in graph_results.iter_mut() {
                let when = match result.valid_from.as_deref() {
                    Some(v) if !v.is_empty() => v.to_string(),
                    _ => result.created_at.clone().unwrap_or_default(),
                };
                if when.is_empty() || window.contains_rfc3339(&when) {
                    continue;
                }
                let meta = result.metadata.get_or_insert_with(Default::default);
                meta.insert("flashback".to_string(), serde_json::Value::Bool(true));
                meta.insert("event_date".to_string(), serde_json::Value::String(when));
                flashbacks += 1;
            }
            if flashbacks > 0 {
                info!(
                    "Window flashbacks: {} expansion rows outside [{:?}..{:?}] flagged",
                    flashbacks, window.from, window.to
                );
            }
        }

        let mut all_results = vector_hits;
        all_results.extend(graph_results);

        let phase3_start = Instant::now();
        let final_results = rank_and_filter(all_results, config.min_combined_score);
        let phase3_duration = phase3_start.elapsed();

        let total_duration = start_time.elapsed();

        {
            let mut stats = self.stats.write().await;
            stats.phase1_duration_ms = phase1_duration.as_millis() as f64;
            stats.phase2_duration_ms = phase2_duration.as_millis() as f64;
            stats.phase3_duration_ms = phase3_duration.as_millis() as f64;
            stats.total_duration_ms = total_duration.as_millis() as f64;
            stats.cache_size = self.cache.read().await.len();
        }

        {
            let mut cache = self.cache.write().await;
            cache.put(
                cache_key,
                CacheEntry {
                    results: final_results.clone(),
                    inserted_at: Instant::now(),
                },
            );
        }

        info!(
            "Smart traversal search completed in {:.2}ms with {} results",
            total_duration.as_millis(),
            final_results.len()
        );

        Ok(final_results)
    }

    pub fn get_stats(&self) -> TraversalStats {
        TraversalStats::default()
    }

    async fn rerank_graph_results(
        &self,
        query_embedding: &[f32],
        graph_results: &mut [SearchResult],
        config: &SearchConfig,
    ) {
        let rerank_start = Instant::now();

        // #88: on a dense graph the expansion can dwarf the final window
        // (observed: 9 seeds -> 1709 rows, all embedded, tens of seconds on
        // local embeddings). Embed only the top rows by pre-rerank score;
        // the tail keeps the neutral 0.5 placeholder and stays reachable —
        // PPR can still lift a deep-but-coherent row. Cost is bounded,
        // reachability is not.
        let scores: Vec<f64> = graph_results.iter().map(|r| r.combined_score).collect();
        let chosen = top_rerank_indices(&scores, config.rerank_max_rows.max(1));
        if chosen.len() < graph_results.len() {
            info!(
                "Re-rank capped: embedding top {} of {} expansion rows (retrieval.rerank_max_rows)",
                chosen.len(),
                graph_results.len()
            );
        }
        let texts: Vec<&str> = chosen
            .iter()
            .map(|&i| graph_results[i].content.as_str())
            .collect();

        match self.embedder.generate_batch(&texts, true).await {
            Ok(embeddings) => {
                for (&i, emb) in chosen.iter().zip(embeddings.iter()) {
                    let result = &mut graph_results[i];
                    let real_sim = cosine_score(query_embedding, emb);
                    result.vector_score = real_sim;
                    result.combined_score = calculate_graph_combined_score_weighted(
                        real_sim,
                        result.graph_score,
                        result.temporal_score,
                        config.graph_semantic_weight,
                        config.graph_graph_weight,
                        config.graph_temporal_weight,
                    );
                }
                info!(
                    "Re-ranked {} graph-expanded results with real cosine in {}ms (algo_opt P0.2)",
                    chosen.len(),
                    rerank_start.elapsed().as_millis()
                );
            }
            Err(e) => {
                warn!(
                    "Graph-result re-rank failed, keeping rank-decay scores: {}",
                    e
                );
            }
        }
    }

    fn make_cache_key(
        &self,
        query: &str,
        query_embedding: &[f32],
        user_id: Option<&str>,
        config: &SearchConfig,
        window: &TimeWindow,
    ) -> String {
        let mut hasher = Sha256::new();

        if self.profile.cache_includes_query_text() {
            hasher.update(query.as_bytes());
        }

        for value in query_embedding {
            hasher.update(value.to_le_bytes());
        }

        if let Some(uid) = user_id {
            hasher.update(uid.as_bytes());
        }

        hasher.update(config.vector_top_k.to_le_bytes());
        hasher.update(config.graph_depth.to_le_bytes());
        hasher.update(config.min_vector_score.to_le_bytes());
        hasher.update(config.min_combined_score.to_le_bytes());
        hasher.update(config.hybrid_vector_weight.to_le_bytes());
        hasher.update(config.hybrid_bm25_weight.to_le_bytes());

        if let Some(edge_types) = &config.edge_types {
            for edge_type in edge_types {
                hasher.update(edge_type.as_bytes());
            }
        }

        if self.profile.cache_correctness_fixes() {
            hasher.update(self.profile.tag().as_bytes());
            for bound in [&window.from, &window.to] {
                match bound {
                    Some(t) => hasher.update(t.timestamp_millis().to_le_bytes()),
                    None => hasher.update(b"open"),
                }
            }
        }

        format!("{:x}", hasher.finalize())
    }
}

/// Blend the real semantic signal with the phase-1 hybrid rank without
/// changing what `metadata.cosine` means to the write-side duplicate gate.
fn rerank_seed(hit: &mut SearchResult, real_cosine: f64, config: &SearchConfig) -> bool {
    let previous_semantic = hit.vector_score;
    let hybrid = hit
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("phase1_hybrid"))
        .is_some();
    let semantic_score = if hybrid {
        let fusion_score = hit
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("rrf_rank_score"))
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(previous_semantic);
        // A document that is strong in only one arm is deliberately placed
        // behind dual-arm documents by RRF.  The fused *position* therefore
        // cannot be the sole lexical relevance signal: a BM25 rank-1 exact
        // hit may sit late in the union and would be erased by cosine
        // reranking.  Preserve the stronger of the fused order and the
        // document's native BM25 rank.  This remains rank-based and avoids
        // mixing incomparable raw BM25/cosine magnitudes.
        let lexical_score = hit
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("bm25_rank_score"))
            .and_then(serde_json::Value::as_f64)
            .map_or(fusion_score, |bm25_score| bm25_score.max(fusion_score));
        blend_hybrid_relevance(
            real_cosine,
            lexical_score,
            config.hybrid_vector_weight,
            config.hybrid_bm25_weight,
        )
    } else {
        real_cosine
    };

    hit.vector_score = semantic_score;
    hit.combined_score = calculate_vector_combined_score_weighted(
        semantic_score,
        hit.temporal_score,
        config.vector_weight,
        config.temporal_weight,
    );
    let metadata = hit.metadata.get_or_insert_with(Default::default);
    metadata.insert("cosine".to_string(), serde_json::Value::from(real_cosine));
    metadata.insert(
        "semantic_score".to_string(),
        serde_json::Value::from(semantic_score),
    );

    (semantic_score - previous_semantic).abs() > 0.01
}

fn blend_hybrid_relevance(
    cosine: f64,
    fusion_score: f64,
    vector_weight: f64,
    bm25_weight: f64,
) -> f64 {
    let vector_weight = vector_weight.max(0.0);
    let bm25_weight = bm25_weight.max(0.0);
    let total = vector_weight + bm25_weight;
    if !total.is_finite() || total <= f64::EPSILON {
        return cosine.clamp(0.0, 1.0);
    }
    ((cosine * vector_weight + fusion_score * bm25_weight) / total).clamp(0.0, 1.0)
}

fn preserve_direct_seed_score(
    source: &str,
    direct_retrieval_score: f64,
    hybrid_semantic_floor: f64,
    graph_blend: f64,
) -> f64 {
    if source == "vector" {
        graph_blend
            .max(direct_retrieval_score)
            .max(hybrid_semantic_floor)
    } else {
        graph_blend
    }
}

/// #88: indices of the top `cap` rows by score, descending. Pure so the
/// selection contract is unit-tested without an embedder.
fn top_rerank_indices(scores: &[f64], cap: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..scores.len()).collect();
    idx.sort_by(|&a, &b| {
        scores[b]
            .partial_cmp(&scores[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    idx.truncate(cap);
    idx
}

#[cfg(test)]
mod tests {
    use super::{
        blend_hybrid_relevance, preserve_direct_seed_score, rerank_seed, top_rerank_indices,
    };
    use crate::toolkit::mind_toolbox::search::smart_traversal::{SearchConfig, SearchResult};
    use std::collections::HashMap;

    #[test]
    fn cap_larger_than_input_keeps_everything() {
        assert_eq!(top_rerank_indices(&[0.1, 0.9, 0.5], 10), vec![1, 2, 0]);
    }

    #[test]
    fn cap_selects_best_scores_not_first_rows() {
        // Discovery order is depth-order — the best candidates may sit late.
        let scores = [0.2, 0.1, 0.95, 0.3, 0.9];
        assert_eq!(top_rerank_indices(&scores, 2), vec![2, 4]);
    }

    #[test]
    fn nan_scores_do_not_panic_or_win() {
        let scores = [f64::NAN, 0.8, 0.4];
        let top = top_rerank_indices(&scores, 2);
        assert_eq!(top.len(), 2);
        assert!(top.contains(&1));
    }

    #[test]
    fn hybrid_seed_keeps_rrf_signal_while_exposing_pure_cosine() {
        let mut hit = SearchResult::from_vector("old", "exact lexical hit", 0.95, 0.0);
        hit.metadata = Some(HashMap::from([
            (
                "phase1_hybrid".to_string(),
                serde_json::Value::String("vector_rrf_bm25".to_string()),
            ),
            ("rrf_rank_score".to_string(), serde_json::Value::from(0.95)),
            ("bm25_rank".to_string(), serde_json::Value::from(1)),
            ("bm25_rank_score".to_string(), serde_json::Value::from(0.95)),
        ]));
        let config = SearchConfig::default();

        rerank_seed(&mut hit, 0.20, &config);

        let expected_semantic = blend_hybrid_relevance(0.20, 0.95, 0.6, 0.4);
        assert!((hit.vector_score - expected_semantic).abs() < 1e-9);
        assert!(hit.combined_score >= config.min_combined_score);
        let metadata = hit.metadata.expect("rerank metadata");
        assert_eq!(
            metadata.get("cosine").and_then(serde_json::Value::as_f64),
            Some(0.20)
        );
        assert_eq!(
            metadata
                .get("bm25_rank")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            metadata
                .get("rrf_rank_score")
                .and_then(serde_json::Value::as_f64),
            Some(0.95)
        );
    }

    #[test]
    fn bm25_rank_one_survives_a_late_rrf_union_position() {
        let mut hit = SearchResult::from_vector("old", "exact lexical hit", 0.20, 0.0);
        hit.metadata = Some(HashMap::from([
            (
                "phase1_hybrid".to_string(),
                serde_json::Value::String("vector_rrf_bm25".to_string()),
            ),
            ("rrf_rank_score".to_string(), serde_json::Value::from(0.20)),
            ("bm25_rank".to_string(), serde_json::Value::from(1)),
            ("bm25_rank_score".to_string(), serde_json::Value::from(0.95)),
        ]));
        let config = SearchConfig::default();

        rerank_seed(&mut hit, 0.50, &config);

        let expected_semantic = blend_hybrid_relevance(0.50, 0.95, 0.6, 0.4);
        assert!((hit.vector_score - expected_semantic).abs() < 1e-9);
        assert!(hit.combined_score >= config.min_combined_score);
    }

    #[test]
    fn non_hybrid_seed_still_uses_pure_cosine() {
        let mut hit = SearchResult::from_vector("plain", "vector hit", 0.95, 0.0);
        let config = SearchConfig::default();

        rerank_seed(&mut hit, 0.42, &config);

        assert!((hit.vector_score - 0.42).abs() < 1e-9);
        assert_eq!(
            hit.metadata
                .as_ref()
                .and_then(|m| m.get("cosine"))
                .and_then(serde_json::Value::as_f64),
            Some(0.42)
        );
    }

    #[test]
    fn direct_seed_score_is_a_floor_for_graph_reranking() {
        assert_eq!(preserve_direct_seed_score("vector", 0.66, 0.0, 0.49), 0.66);
        assert_eq!(preserve_direct_seed_score("vector", 0.40, 0.0, 0.70), 0.70);
        assert_eq!(preserve_direct_seed_score("graph", 0.66, 0.95, 0.49), 0.49);
    }

    #[test]
    fn bm25_hybrid_semantic_is_a_direct_seed_floor() {
        assert_eq!(preserve_direct_seed_score("vector", 0.66, 0.78, 0.49), 0.78);
    }
}
