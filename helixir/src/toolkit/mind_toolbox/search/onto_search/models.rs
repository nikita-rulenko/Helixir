

use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptMatch {
    pub concept_id: String,
    pub confidence: f64,
    pub match_type: String,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagMatch {
    pub tag: String,
    pub score: f64,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphContext {
    pub related_memories: Vec<String>,
    pub edge_types: Vec<String>,
    pub edge_weights: Vec<f64>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntoSearchResult {
    pub memory_id: String,
    pub content: String,
    pub memory_type: String,
    pub user_id: String,
    pub vector_score: f64,
    pub concept_score: f64,
    pub tag_score: f64,
    pub graph_score: f64,
    pub temporal_score: f64,
    pub final_score: f64,
    pub matched_concepts: Vec<ConceptMatch>,
    pub matched_tags: Vec<TagMatch>,
    pub graph_context: Option<GraphContext>,
    pub created_at: String,
    pub depth: usize,
    pub source: String,
}

impl Default for OntoSearchResult {
    fn default() -> Self {
        Self {
            memory_id: String::new(),
            content: String::new(),
            memory_type: String::new(),
            user_id: String::new(),
            vector_score: 0.0,
            concept_score: 0.0,
            tag_score: 0.0,
            graph_score: 0.0,
            temporal_score: 0.0,
            final_score: 0.0,
            matched_concepts: Vec::new(),
            matched_tags: Vec::new(),
            graph_context: None,
            created_at: String::new(),
            depth: 0,
            source: "vector".to_string(),
        }
    }
}

