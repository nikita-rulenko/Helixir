//! [`SearchEngine`] — facade that owns vector / BM25 / hybrid / smart-traversal
//! backends and exposes the unified search surface to callers.
//!
//! The top-level dispatch ([`SearchEngine::search`], [`SearchEngine::search_for_dedup`])
//! lives in [`super::dispatch`]; collective enrichment (`user_count`, controversy)
//! lives in [`super::enrichment`]. This file owns construction, configuration
//! helpers and the thin per-backend facades.

use std::sync::Arc;

use moka::future::Cache as MokaCache;
use sha2::{Digest, Sha256};
use tracing::debug;

use crate::db::HelixClient;
use crate::llm::EmbeddingGenerator;

use super::bm25::Bm25Search;
use super::hybrid::{HybridSearch, HybridSearchError};
use super::models::SearchResult;
use super::smart_traversal_v2::{SearchConfig, SmartTraversalV2};
use super::types::{SearchEngineConfig, UnifiedSearchResult};
use super::vector::{VectorSearch, VectorSearchError};

pub struct SearchEngine {
    pub(super) client: Arc<HelixClient>,
    pub(super) vector: Arc<VectorSearch>,
    pub(super) hybrid: HybridSearch,
    pub(super) smart_traversal: Option<SmartTraversalV2>,
    pub(super) config: SearchEngineConfig,
    pub(super) cross_user_cache: MokaCache<String, Vec<UnifiedSearchResult>>,
}

pub(super) fn embedding_cache_key(embedding: &[f32]) -> String {
    let mut hasher = Sha256::new();
    for &v in embedding {
        hasher.update(v.to_le_bytes());
    }
    format!("{:x}", hasher.finalize())
}

impl SearchEngine {
    pub fn new(
        client: Arc<HelixClient>,
        embedder: Arc<EmbeddingGenerator>,
        config: SearchEngineConfig,
    ) -> Self {
        let vector = Arc::new(VectorSearch::new(
            Arc::clone(&client),
            config.cache_size,
            config.cache_ttl,
        ));
        let hybrid = HybridSearch::new(vector.clone(), config.vector_weight, config.bm25_weight);
        let smart_traversal = if config.enable_smart_traversal {
            Some(SmartTraversalV2::new(
                Arc::clone(&client),
                Arc::clone(&embedder),
                config.cache_size,
                config.cache_ttl,
            ))
        } else {
            None
        };
        let cross_user_cache = MokaCache::builder()
            .max_capacity(1000)
            .time_to_live(std::time::Duration::from_secs(60))
            .build();
        Self {
            client,
            vector,
            hybrid,
            smart_traversal,
            config,
            cross_user_cache,
        }
    }

    pub(super) fn make_search_config(
        &self,
        vector_top_k: usize,
        graph_depth: u32,
        min_vector_score: f64,
        min_combined_score: f64,
        temporal_weight: f64,
    ) -> SearchConfig {
        let t = &self.config.search_thresholds;
        let _ = t.temporal_weight; // superseded by the per-mode weight (#31)
        SearchConfig {
            vector_top_k,
            graph_depth,
            beam_width: self.config.retrieval.graph.expansion_children_per_parent,
            min_vector_score,
            min_combined_score,
            edge_types: Some(vec![
                "BECAUSE".to_string(),
                "IMPLIES".to_string(),
                "MEMORY_RELATION".to_string(),
            ]),
            vector_weight: t.vector_weight,
            temporal_weight,
            graph_semantic_weight: t.graph_semantic_weight,
            graph_graph_weight: t.graph_graph_weight,
            graph_temporal_weight: t.graph_temporal_weight,
            temporal_decay_days: t.default_temporal_days,
            ppr_alpha: self.config.retrieval.ppr.alpha,
            ppr_iterations: self.config.retrieval.ppr.max_iterations,
            rank_base: self.config.retrieval.rank_base,
            rank_decay: self.config.retrieval.rank_decay,
            edge_weights: self.config.retrieval.graph.edge_weights,
            edge_damping: self.config.retrieval.graph.edge_damping,
        }
    }

    pub(super) async fn vector_search_unified(
        &self,
        query: &str,
        user_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<UnifiedSearchResult>, super::types::SearchError> {
        let vector_results = self.vector.search(query, user_id, limit, 0.0, true).await?;

        Ok(vector_results
            .into_iter()
            .map(|r| UnifiedSearchResult {
                memory_id: r.memory_id,
                content: r.content,
                score: r.score as f32,
                method: "vector".to_string(),
                metadata: r.metadata,
                created_at: r.created_at,
                user_count: None,
                controversy: None,
            })
            .collect())
    }

    // `query_embedding` is accepted but unused: the underlying vector layer
    // re-embeds `query` itself via its own cache. The parameter is preserved
    // for the upcoming external-embedding fast-path (see issue #6 follow-up).
    pub async fn vector_search(
        &self,
        query: &str,
        _query_embedding: &[f32],
        user_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchResult>, VectorSearchError> {
        self.vector.search(query, user_id, limit, 0.0, true).await
    }

    pub fn bm25_search(
        &self,
        query: &str,
        documents: &[(String, String)],
        limit: usize,
    ) -> Vec<SearchResult> {
        Bm25Search::search(query, documents, limit, 0.0)
    }

    pub async fn hybrid_search(
        &self,
        query: &str,
        user_id: Option<&str>,
        documents: Option<&[(String, String)]>,
        limit: usize,
    ) -> Result<Vec<SearchResult>, HybridSearchError> {
        self.hybrid.search(query, user_id, documents, limit).await
    }

    pub fn cache_stats(&self) -> super::cache::CacheStats {
        super::cache::CacheStats::default()
    }

    pub fn clear_cache(&self) {
        debug!("SearchEngine.clear_cache: noop (caches are TTL-managed)");
    }
}
