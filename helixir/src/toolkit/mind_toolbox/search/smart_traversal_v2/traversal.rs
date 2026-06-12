use super::batch_expansion::graph_expansion_phase_batched;
use super::ppr::personalized_pagerank;
use super::models::{SearchConfig, SearchResult, TraversalStats};
use super::phases::{TraversalError, graph_expansion_phase, rank_and_filter, vector_search_phase};
use super::scoring::{calculate_graph_combined_score_weighted, cosine_score};
use crate::core::RetrievalProfile;
use crate::db::HelixClient;
use crate::llm::EmbeddingGenerator;
use chrono::{DateTime, Utc};
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
        temporal_cutoff: Option<DateTime<Utc>>,
    ) -> Result<Vec<SearchResult>, TraversalError> {
        let cache_key = self.make_cache_key(
            query,
            query_embedding,
            user_id,
            &config,
            temporal_cutoff,
        );

        {
            let mut cache = self.cache.write().await;
            if let Some(entry) = cache.get(&cache_key) {
                let ttl_ok = !self.profile.cache_correctness_fixes()
                    || entry.inserted_at.elapsed() < self.cache_ttl;
                if ttl_ok {
                    let cached_results = entry.results.clone();
                    let mut stats = self.stats.write().await;
                    stats.cache_hits += 1;
                    stats.cache_hit_rate = stats.cache_hits as f64
                        / (stats.cache_hits + stats.cache_misses) as f64;
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
            temporal_cutoff,
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
                    if (real_score - hit.vector_score).abs() > 0.01 {
                        let temporal = hit.temporal_score;
                        hit.vector_score = real_score;
                        hit.combined_score = (real_score * config.vector_weight
                            + temporal * config.temporal_weight)
                            .clamp(0.0, 1.0);
                        reranked += 1;
                    }
                }
                vector_hits
                    .sort_by(|a, b| b.combined_score.partial_cmp(&a.combined_score).unwrap());
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
            let results = graph_expansion_phase(
                Arc::clone(&self.client),
                &vector_hits,
                query_embedding,
                &config,
            )
            .await?;
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
            let ppr_scores = personalized_pagerank(&ego_edges, &personalization);
            let mut rescored = 0usize;
            for result in vector_hits.iter_mut().chain(graph_results.iter_mut()) {
                let Some(ppr) = ppr_scores.get(&result.memory_id) else {
                    continue;
                };
                result.graph_score = *ppr;
                result.combined_score = (config.graph_semantic_weight * result.vector_score
                    + config.graph_graph_weight * ppr
                    + config.graph_temporal_weight * result.temporal_score)
                    .clamp(0.0, 1.0);
                if let Some(meta) = result.metadata.as_mut() {
                    meta.insert(
                        "ppr".to_string(),
                        serde_json::Value::from((ppr * 1000.0).round() / 1000.0),
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
        let texts: Vec<&str> = graph_results.iter().map(|r| r.content.as_str()).collect();

        match self.embedder.generate_batch(&texts, true).await {
            Ok(embeddings) => {
                for (result, emb) in graph_results.iter_mut().zip(embeddings.iter()) {
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
                    graph_results.len(),
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
        temporal_cutoff: Option<DateTime<Utc>>,
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

        if let Some(edge_types) = &config.edge_types {
            for edge_type in edge_types {
                hasher.update(edge_type.as_bytes());
            }
        }

        if self.profile.cache_correctness_fixes() {
            hasher.update(self.profile.tag().as_bytes());
            if let Some(cutoff) = temporal_cutoff {
                hasher.update(cutoff.timestamp_millis().to_le_bytes());
            } else {
                hasher.update(b"no-cutoff");
            }
        }

        format!("{:x}", hasher.finalize())
    }
}
