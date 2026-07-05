use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
        Self::from_vector_weighted(memory_id, content, vector_score, temporal_score, 0.7, 0.3)
    }

    pub fn from_vector_weighted(
        memory_id: impl Into<String>,
        content: impl Into<String>,
        vector_score: f64,
        temporal_score: f64,
        vector_weight: f64,
        temporal_weight: f64,
    ) -> Self {
        let combined = super::scoring::calculate_vector_combined_score_weighted(
            vector_score,
            temporal_score,
            vector_weight,
            temporal_weight,
        );
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
        Self::from_graph_weighted(
            memory_id,
            content,
            semantic_sim,
            graph_score,
            temporal_score,
            depth,
            edge_path,
            0.3,
            0.5,
            0.2,
        )
    }

    pub fn from_graph_weighted(
        memory_id: impl Into<String>,
        content: impl Into<String>,
        semantic_sim: f64,
        graph_score: f64,
        temporal_score: f64,
        depth: u32,
        edge_path: Vec<String>,
        semantic_weight: f64,
        graph_weight: f64,
        temporal_weight: f64,
    ) -> Self {
        let combined = super::scoring::calculate_graph_combined_score_weighted(
            semantic_sim,
            graph_score,
            temporal_score,
            semantic_weight,
            graph_weight,
            temporal_weight,
        );
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

    /// #36: children per parent that survive into the next expansion
    /// frontier (was a hardcoded top-3). Sourced from
    /// retrieval.graph.expansion_children_per_parent.
    pub beam_width: usize,

    pub min_vector_score: f64,

    pub min_combined_score: f64,

    pub edge_types: Option<Vec<String>>,

    pub vector_weight: f64,
    pub temporal_weight: f64,
    pub graph_semantic_weight: f64,
    pub graph_graph_weight: f64,
    pub graph_temporal_weight: f64,
    pub temporal_decay_days: f64,

    // Ranking knobs sourced from config.retrieval (was hardcoded in ppr.rs /
    // phases.rs). Defaults here mirror those consts.
    pub ppr_alpha: f64,
    pub ppr_iterations: usize,
    pub rank_base: f64,
    pub rank_decay: f64,
    pub edge_weights: crate::core::config::EdgeWeights,
    pub edge_damping: crate::core::config::EdgeDamping,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            vector_top_k: 10,
            graph_depth: 2,
            beam_width: 3,
            min_vector_score: 0.5,
            min_combined_score: 0.3,
            edge_types: Some(vec![
                "BECAUSE".to_string(),
                "IMPLIES".to_string(),
                "MEMORY_RELATION".to_string(),
            ]),
            vector_weight: 0.7,
            temporal_weight: 0.3,
            graph_semantic_weight: 0.3,
            graph_graph_weight: 0.5,
            graph_temporal_weight: 0.2,
            temporal_decay_days: 30.0,
            ppr_alpha: 0.6,
            ppr_iterations: 20,
            rank_base: 0.95,
            rank_decay: 0.92,
            edge_weights: crate::core::config::EdgeWeights::default(),
            edge_damping: crate::core::config::EdgeDamping::default(),
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
