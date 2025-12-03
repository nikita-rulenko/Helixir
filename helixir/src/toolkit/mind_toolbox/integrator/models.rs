use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use strum::{EnumString, IntoStaticStr};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarMemory {
    pub memory_id: String,
    pub content: String,
    pub embedding: Vec<f32>,
    pub similarity_score: f64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, EnumString, IntoStaticStr)]
pub enum RelationType {
    Supersedes,
    Implies,
    Because,
    Contradicts,
    RelatesTo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRelation {
    pub target_id: String,
    pub relation_type: RelationType,
    pub confidence: f64,
    pub reasoning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationResult {
    pub memory_id: String,
    pub similar_found: usize,
    pub relations_created: usize,
    pub superseded_memories: Vec<String>,
    pub integration_time_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationConfig {
    #[serde(default = "default_similarity_threshold")]
    pub similarity_threshold: f64,
    #[serde(default = "default_max_similar")]
    pub max_similar: usize,
    #[serde(default = "default_enable_reasoning")]
    pub enable_reasoning: bool,
}

impl Default for IntegrationConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.7,
            max_similar: 10,
            enable_reasoning: true,
        }
    }
}

fn default_similarity_threshold() -> f64 {
    0.7
}

fn default_max_similar() -> usize {
    10
}

fn default_enable_reasoning() -> bool {
    true
}