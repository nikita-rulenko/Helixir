

use serde::{Deserialize, Serialize};
use std::collections::HashMap;


pub mod edge_weights {
    pub const BECAUSE: f64 = 1.0;
    pub const IMPLIES: f64 = 0.9;
    pub const SIMILAR_TO: f64 = 0.75;
    pub const MEMORY_RELATION: f64 = 0.7;
    pub const EXTRACTED_ENTITY: f64 = 0.6;
    pub const CONTRADICTS: f64 = 0.4;
    pub const DEFAULT: f64 = 0.5;

    
    pub fn get_weight(edge_type: &str) -> f64 {
        match edge_type.to_uppercase().as_str() {
            "BECAUSE" => BECAUSE,
            "IMPLIES" => IMPLIES,
            "SIMILAR_TO" => SIMILAR_TO,
            "MEMORY_RELATION" => MEMORY_RELATION,
            "EXTRACTED_ENTITY" => EXTRACTED_ENTITY,
            "CONTRADICTS" => CONTRADICTS,
            _ => DEFAULT,
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    
    pub memory_id: String,
    
    pub content: String,
    
    pub vector_score: f64,
    
    pub graph_score: f64,
    
    pub temporal_score: f64,
    
    pub combined_score: f64,
    
    pub depth: u32,
    
    pub source: String,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_path: Option<Vec<String>>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

impl SearchResult {
    
    pub fn from_vector(
        memory_id: impl Into<String>,
        content: impl Into<String>,
        vector_score: f64,
        temporal_score: f64,
    ) -> Self {
        let combined = vector_score * 0.7 + temporal_score * 0.3;
        Self {
            memory_id: memory_id.into(),
            content: content.into(),
            vector_score,
            graph_score: 0.0,
            temporal_score,
            combined_score: combined,
            depth: 0,
            source: "vector".to_string(),
            edge_path: None,
            metadata: None,
            created_at: None,
        }
    }

    
    pub fn from_graph(
        memory_id: impl Into<String>,
        content: impl Into<String>,
        semantic_sim: f64,
        graph_score: f64,
        temporal_score: f64,
        depth: u32,
        edge_path: Vec<String>,
    ) -> Self {
        
        let combined = semantic_sim * 0.3 + graph_score * 0.5 + temporal_score * 0.2;
        Self {
            memory_id: memory_id.into(),
            content: content.into(),
            vector_score: semantic_sim,
            graph_score,
            temporal_score,
            combined_score: combined,
            depth,
            source: "graph".to_string(),
            edge_path: Some(edge_path),
            metadata: None,
            created_at: None,
        }
    }

    
    pub fn with_metadata(mut self, metadata: HashMap<String, serde_json::Value>) -> Self {
        self.metadata = Some(metadata);
        self
    }
}


#[derive(Debug, Clone)]
pub struct SearchConfig {
    
    pub vector_top_k: usize,
    
    pub graph_depth: u32,
    
    pub min_vector_score: f64,
    
    pub min_combined_score: f64,
    
    pub edge_types: Option<Vec<String>>,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            vector_top_k: 10,
            graph_depth: 2,
            min_vector_score: 0.5,
            min_combined_score: 0.3,
            edge_types: Some(vec![
                "BECAUSE".to_string(),
                "IMPLIES".to_string(),
                "MEMORY_RELATION".to_string(),
            ]),
        }
    }
}


#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraversalStats {
    pub cache_size: usize,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_hit_rate: f64,
    pub phase1_duration_ms: f64,
    pub phase2_duration_ms: f64,
    pub phase3_duration_ms: f64,
    pub total_duration_ms: f64,
}

