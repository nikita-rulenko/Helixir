

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

use crate::core::config::SearchThresholds;
use crate::db::HelixClient;
use crate::llm::EmbeddingGenerator;
use crate::core::search_modes::SearchMode;
use smart_traversal_v2::models::SearchConfig;
use std::collections::HashMap;
use std::sync::Arc;
use chrono::{DateTime, Utc, Duration};
use tracing::{debug, info};
use moka::future::Cache as MokaCache;
use sha2::{Sha256, Digest};


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
    pub search_thresholds: SearchThresholds,
}

impl Default for SearchEngineConfig {
    fn default() -> Self {
        Self {
            cache_size: 500,
            cache_ttl: 300,
            enable_smart_traversal: true,
            vector_weight: 0.6,
            bm25_weight: 0.4,
            search_thresholds: SearchThresholds::default(),
        }
    }
}


#[derive(Debug, Clone, serde::Serialize)]
pub struct ControversyInfo {
    pub conflicting_memory_id: String,
    pub conflicting_content: String,
    pub conflicting_user_id: String,
    pub conflict_type: String,
}

#[derive(Debug, Clone)]
pub struct UnifiedSearchResult {
    pub memory_id: String,
    pub content: String,
    pub score: f32,
    pub method: String,
    pub metadata: HashMap<String, serde_json::Value>,
    pub created_at: String,
    pub user_count: Option<u32>,
    pub controversy: Option<ControversyInfo>,
}

pub struct SearchEngine {
    client: Arc<HelixClient>,
    vector: Arc<VectorSearch>,
    hybrid: HybridSearch,
    smart_traversal: Option<SmartTraversalV2>,
    config: SearchEngineConfig,
    cross_user_cache: MokaCache<String, Vec<UnifiedSearchResult>>,
}

fn embedding_cache_key(embedding: &[f32]) -> String {
    let mut hasher = Sha256::new();
    for &v in embedding {
        hasher.update(v.to_le_bytes());
    }
    format!("{:x}", hasher.finalize())
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
        let cross_user_cache = MokaCache::builder()
            .max_capacity(1000)
            .time_to_live(std::time::Duration::from_secs(60))
            .build();
        Self { client, vector, hybrid, smart_traversal, config, cross_user_cache }
    }

    
    pub async fn search(
        &self,
        query: &str,
        query_embedding: &[f32],
        user_id: &str,
        limit: usize,
        mode: &str,
        temporal_days: Option<f64>,
        scope: &str,
    ) -> Result<Vec<UnifiedSearchResult>, SearchError> {
        
        let query_preview: String = query.chars().take(30).collect();
        
        
        let search_mode = SearchMode::parse_mode(mode);
        let mode_defaults = search_mode.get_defaults();
        let effective_temporal_days = temporal_days.or(mode_defaults.temporal_days);
        
        let temporal_cutoff: Option<DateTime<Utc>> = effective_temporal_days.map(|days| {
            let millis = (days * 24.0 * 60.0 * 60.0 * 1000.0) as i64;
            Utc::now() - Duration::milliseconds(millis)
        });

        let effective_user_id: Option<&str> = match scope {
            "collective" | "all" => None,
            _ => Some(user_id),
        };
        
        info!(
            "SearchEngine.search: query='{}...', user={}, mode={}, limit={}, scope={}, temporal_days={:?}", 
            query_preview, user_id, mode, limit, scope, effective_temporal_days
        );

        if effective_user_id.is_none() {
            let cache_key = embedding_cache_key(query_embedding);
            if let Some(cached) = self.cross_user_cache.get(&cache_key).await {
                info!("Cross-user cache hit for scope={}", scope);
                return Ok(cached);
            }
        }

        let results = match mode.to_lowercase().as_str() {
            "recent" | "contextual" => {
                
                if let Some(ref traversal) = self.smart_traversal {
                    debug!(
                        "Using SmartTraversalV2 for mode={}, temporal_cutoff={:?}, scope={}", 
                        mode, temporal_cutoff, scope
                    );
                    let config = self.make_search_config(
                        limit,
                        if mode == "recent" { 1 } else { 2 },
                        mode_defaults.min_vector_score,
                        mode_defaults.min_combined_score,
                    );
                    let traversal_results = traversal
                        .search(query, query_embedding, effective_user_id, config, temporal_cutoff)
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
                            user_count: None,
                            controversy: None,
                        })
                        .collect()
                } else {
                    
                    self.vector_search_unified(query, effective_user_id, limit).await?
                }
            }
            "deep" => {
                
                if let Some(ref traversal) = self.smart_traversal {
                    debug!(
                        "Using SmartTraversalV2 for deep search, temporal_cutoff={:?}, scope={}", 
                        temporal_cutoff, scope
                    );
                    let config = self.make_search_config(
                        limit * 2,
                        3,
                        self.config.search_thresholds.min_vector_score,
                        mode_defaults.min_combined_score,
                    );
                    let traversal_results = traversal
                        .search(query, query_embedding, effective_user_id, config, temporal_cutoff)
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
                            user_count: None,
                            controversy: None,
                        })
                        .collect()
                } else {
                    self.vector_search_unified(query, effective_user_id, limit).await?
                }
            }
            "full" => {
                
                if let Some(ref traversal) = self.smart_traversal {
                    debug!("Using SmartTraversalV2 for full mode (no temporal filter), scope={}", scope);
                    let config = self.make_search_config(
                        limit * 2,
                        4,
                        self.config.search_thresholds.min_vector_score,
                        self.config.search_thresholds.min_combined_score,
                    );
                    let traversal_results = traversal
                        .search(query, query_embedding, effective_user_id, config, None)
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
                            user_count: None,
                            controversy: None,
                        })
                        .collect()
                } else {
                    debug!("SmartTraversal not available, returning empty for full mode");
                    Vec::new()
                }
            }
            _ => {
                
                debug!("Unknown mode '{}', falling back to vector search", mode);
                self.vector_search_unified(query, effective_user_id, limit).await?
            }
        };

        let mut final_results = results;

        if (scope == "collective" || scope == "all") && !final_results.is_empty() {
            for result in &mut final_results {
                if let Ok(user_count) = self.fetch_memory_user_count(&result.memory_id).await {
                    result.user_count = Some(user_count);
                }
                if let Ok(controversy) = self.fetch_controversy(&result.memory_id, user_id).await {
                    result.controversy = controversy;
                }
            }
        }

        if effective_user_id.is_none() {
            let cache_key = embedding_cache_key(query_embedding);
            self.cross_user_cache.insert(cache_key, final_results.clone()).await;
        }

        info!("SearchEngine.search complete: {} results (scope={})", final_results.len(), scope);
        Ok(final_results)
    }

    async fn fetch_memory_user_count(&self, memory_id: &str) -> Result<u32, SearchError> {
        #[derive(serde::Deserialize)]
        struct UsersResult {
            #[serde(default)]
            users: Vec<serde_json::Value>,
        }

        let result: UsersResult = self.client
            .execute_query("getMemoryUsers", &serde_json::json!({"memory_id": memory_id}))
            .await
            .map_err(|e| SearchError::InvalidMode(e.to_string()))?;

        Ok(result.users.len().max(1) as u32)
    }

    async fn fetch_controversy(
        &self,
        memory_id: &str,
        current_user_id: &str,
    ) -> Result<Option<ControversyInfo>, SearchError> {
        #[derive(serde::Deserialize)]
        struct ContradictionsResult {
            #[serde(default)]
            contradicts_out: Vec<ContradictedMemory>,
            #[serde(default)]
            contradicts_in: Vec<ContradictedMemory>,
        }

        #[derive(serde::Deserialize)]
        struct ContradictedMemory {
            #[serde(default)]
            memory_id: String,
            #[serde(default)]
            content: String,
            #[serde(default)]
            user_id: String,
        }

        let result: ContradictionsResult = self.client
            .execute_query("getMemoryContradictions", &serde_json::json!({"memory_id": memory_id}))
            .await
            .map_err(|e| SearchError::InvalidMode(e.to_string()))?;

        let all_contradictions: Vec<&ContradictedMemory> = result.contradicts_out.iter()
            .chain(result.contradicts_in.iter())
            .filter(|m| !m.memory_id.is_empty() && m.user_id != current_user_id)
            .collect();

        if let Some(conflict) = all_contradictions.first() {
            Ok(Some(ControversyInfo {
                conflicting_memory_id: conflict.memory_id.clone(),
                conflicting_content: conflict.content.clone(),
                conflicting_user_id: conflict.user_id.clone(),
                conflict_type: "preference_conflict".to_string(),
            }))
        } else {
            Ok(None)
        }
    }

    
    fn make_search_config(
        &self,
        vector_top_k: usize,
        graph_depth: u32,
        min_vector_score: f64,
        min_combined_score: f64,
    ) -> SearchConfig {
        let t = &self.config.search_thresholds;
        SearchConfig {
            vector_top_k,
            graph_depth,
            min_vector_score,
            min_combined_score,
            edge_types: Some(vec![
                "BECAUSE".to_string(),
                "IMPLIES".to_string(),
                "MEMORY_RELATION".to_string(),
            ]),
            vector_weight: t.vector_weight,
            temporal_weight: t.temporal_weight,
            graph_semantic_weight: t.graph_semantic_weight,
            graph_graph_weight: t.graph_graph_weight,
            graph_temporal_weight: t.graph_temporal_weight,
            temporal_decay_days: t.default_temporal_days,
        }
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
                user_count: None,
                controversy: None,
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
