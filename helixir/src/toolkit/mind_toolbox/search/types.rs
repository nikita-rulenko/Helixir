//! Public types of the `search` facade: error, engine config and the unified
//! result shape consumed by [`crate::toolkit::tooling_manager::ToolingManager`].

use std::collections::HashMap;

use crate::core::config::{RetrievalConfig, SearchThresholds};

use super::hybrid::HybridSearchError;
use super::vector::VectorSearchError;

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
    pub retrieval: RetrievalConfig,
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
            retrieval: RetrievalConfig::default(),
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
