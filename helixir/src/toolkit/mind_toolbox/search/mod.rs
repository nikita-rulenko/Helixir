

pub mod models;
pub mod cache;
pub mod vector;
pub mod bm25;
pub mod hybrid;
pub mod smart_traversal_v2;
pub mod onto_search;
pub mod query_processor;

pub use models::{SearchResult, SearchMethod};
pub use cache::{SearchCache, CacheStats};
pub use vector::{VectorSearch, VectorSearchError};
pub use bm25::Bm25Search;
pub use hybrid::{HybridSearch, HybridSearchError};


pub use smart_traversal_v2::{
    SmartTraversalV2,
    SearchConfig as SmartSearchConfig,
    cosine_similarity,
    calculate_temporal_freshness,
    edge_weights,
};


pub use onto_search::{
    OntoSearchConfig,
    OntoSearchResult,
    parse_datetime_utc,
    is_within_temporal_window,
    calculate_temporal_freshness as onto_temporal_freshness,
};


pub use query_processor::{QueryProcessor, QueryIntent, EnhancedQuery};

use crate::db::HelixClient;
use crate::llm::EmbeddingGenerator;
use crate::core::search_modes::SearchMode;
use smart_traversal_v2::models::SearchConfig;
use std::collections::HashMap;
use std::sync::Arc;
use chrono::{DateTime, Utc, Duration};
use tracing::{debug, info};


#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("Vector search failed: {0}")]
    Vector(#[from] VectorSearchError),
    #[error("Hybrid search failed: {0}")]
    Hybrid(#[from] HybridSearchError),
    #[error("Invalid mode: {0}")]
    InvalidMode(String),
}

#[derive(Debug, Clone)]
pub struct SearchEngineConfig {
    pub cache_size: usize,
    pub cache_ttl: u64,
    pub enable_smart_traversal: bool,
    pub vector_weight: f64,
    pub bm25_weight: f64,
}

impl Default for SearchEngineConfig {
    fn default() -> Self {
        Self {
            cache_size: 500,
            cache_ttl: 300,
            enable_smart_traversal: true,
            vector_weight: 0.6,
            bm25_weight: 0.4,
        }
    }
}


#[derive(Debug, Clone)]
pub struct UnifiedSearchResult {
    pub memory_id: String,
    pub content: String,
    pub score: f32,
    pub method: String,
    pub metadata: HashMap<String, serde_json::Value>,
    pub created_at: String,
}

pub struct SearchEngine {
    client: Arc<HelixClient>,
    vector: Arc<VectorSearch>,
    hybrid: HybridSearch,
    smart_traversal: Option<SmartTraversalV2>,
    config: SearchEngineConfig,
}

impl SearchEngine {
    pub fn new(
        client: Arc<HelixClient>, 
        _embedder: Arc<EmbeddingGenerator>,
        config: SearchEngineConfig,
    ) -> Self {
        let vector = Arc::new(VectorSearch::new(Arc::clone(&client), config.cache_size, config.cache_ttl));
        let hybrid = HybridSearch::new(vector.clone(), config.vector_weight, config.bm25_weight);
        let smart_traversal = if config.enable_smart_traversal {
            Some(SmartTraversalV2::new(Arc::clone(&client), config.cache_size, config.cache_ttl))
        } else {
            None
        };
        Self { client, vector, hybrid, smart_traversal, config }
    }

    
    pub async fn search(
        &self,
        query: &str,
        query_embedding: &[f32],
        user_id: &str,
        limit: usize,
        mode: &str,
        temporal_days: Option<f64>,
    ) -> Result<Vec<UnifiedSearchResult>, SearchError> {
        
        let query_preview: String = query.chars().take(30).collect();
        
        
        let search_mode = SearchMode::from_str(mode);
        let mode_defaults = search_mode.get_defaults();
        let effective_temporal_days = temporal_days.or(mode_defaults.temporal_days);
        
        let temporal_cutoff: Option<DateTime<Utc>> = effective_temporal_days.map(|days| {
            let millis = (days * 24.0 * 60.0 * 60.0 * 1000.0) as i64;
            Utc::now() - Duration::milliseconds(millis)
        });
        
        info!(
            "SearchEngine.search: query='{}...', user={}, mode={}, limit={}, temporal_days={:?}", 
            query_preview, user_id, mode, limit, effective_temporal_days
        );

        let results = match mode.to_lowercase().as_str() {
            "recent" | "contextual" => {
                
                if let Some(ref traversal) = self.smart_traversal {
                    debug!(
                        "Using SmartTraversalV2 for mode={}, temporal_cutoff={:?}", 
                        mode, temporal_cutoff
                    );
                    let config = SearchConfig {
                        vector_top_k: limit,
                        graph_depth: if mode == "recent" { 1 } else { 2 },
                        min_vector_score: mode_defaults.min_vector_score,
                        min_combined_score: mode_defaults.min_combined_score,
                        ..Default::default()
                    };
                    let traversal_results = traversal
                        .search(query, query_embedding, Some(user_id), config, temporal_cutoff)
                        .await
                        .unwrap_or_default();
                    
                    traversal_results
                        .into_iter()
                        .map(|r| UnifiedSearchResult {
                            memory_id: r.memory_id,
                            content: r.content,
                            score: r.combined_score as f32,
                            method: format!("smart_v2_{}", mode),
                            metadata: r.metadata.unwrap_or_default(),
                            created_at: r.created_at.unwrap_or_default(),
                        })
                        .collect()
                } else {
                    
                    self.vector_search_unified(query, Some(user_id), limit).await?
                }
            }
            "deep" => {
                
                if let Some(ref traversal) = self.smart_traversal {
                    debug!(
                        "Using SmartTraversalV2 for deep search, temporal_cutoff={:?}", 
                        temporal_cutoff
                    );
                    let config = SearchConfig {
                        vector_top_k: limit * 2,
                        graph_depth: 3,
                        min_combined_score: mode_defaults.min_combined_score,
                        ..Default::default()
                    };
                    let traversal_results = traversal
                        .search(query, query_embedding, Some(user_id), config, temporal_cutoff)
                        .await
                        .unwrap_or_default();
                    
                    traversal_results
                        .into_iter()
                        .take(limit)
                        .map(|r| UnifiedSearchResult {
                            memory_id: r.memory_id,
                            content: r.content,
                            score: r.combined_score as f32,
                            method: "smart_v2_deep".to_string(),
                            metadata: r.metadata.unwrap_or_default(),
                            created_at: r.created_at.unwrap_or_default(),
                        })
                        .collect()
                } else {
                    self.vector_search_unified(query, Some(user_id), limit).await?
                }
            }
            "full" => {
                
                if let Some(ref traversal) = self.smart_traversal {
                    debug!("Using SmartTraversalV2 for full mode (no temporal filter)");
                    let config = SearchConfig {
                        vector_top_k: limit * 2,
                        graph_depth: 4,
                        min_combined_score: 0.3,
                        ..Default::default()
                    };
                    let traversal_results = traversal
                        .search(query, query_embedding, Some(user_id), config, None)
                        .await
                        .unwrap_or_default();
                    
                    traversal_results
                        .into_iter()
                        .take(limit)
                        .map(|r| UnifiedSearchResult {
                            memory_id: r.memory_id,
                            content: r.content,
                            score: r.combined_score as f32,
                            method: "smart_v2_full".to_string(),
                            metadata: r.metadata.unwrap_or_default(),
                            created_at: r.created_at.unwrap_or_default(),
                        })
                        .collect()
                } else {
                    debug!("SmartTraversal not available, returning empty for full mode");
                    Vec::new()
                }
            }
            _ => {
                
                debug!("Unknown mode '{}', falling back to vector search", mode);
                self.vector_search_unified(query, Some(user_id), limit).await?
            }
        };

        info!("SearchEngine.search complete: {} results", results.len());
        Ok(results)
    }

    
    async fn vector_search_unified(
        &self,
        query: &str,
        user_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<UnifiedSearchResult>, SearchError> {
        let vector_results = self.vector
            .search(query, user_id, limit, 0.0, true)
            .await?;
        
        Ok(vector_results
            .into_iter()
            .map(|r| UnifiedSearchResult {
                memory_id: r.memory_id,
                content: r.content,
                score: r.score as f32,
                method: "vector".to_string(),
                metadata: r.metadata,
                created_at: r.created_at,
            })
            .collect())
    }

    
    pub async fn vector_search(
        &self,
        query: &str,
        query_embedding: &[f32],
        user_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchResult>, VectorSearchError> {
        self.vector.search(query, user_id, limit, 0.0, true).await
    }

    
    pub fn bm25_search(&self, query: &str, documents: &[(String, String)], limit: usize) -> Vec<SearchResult> {
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

    
    pub fn cache_stats(&self) -> CacheStats {
        CacheStats::default()
    }

    
    pub fn clear_cache(&self) {
        
    }
}
