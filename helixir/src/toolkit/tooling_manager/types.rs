use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::toolkit::mind_toolbox::chunking::ChunkingError;
use crate::toolkit::mind_toolbox::entity::EntityError;
use crate::toolkit::mind_toolbox::ontology::OntologyError;
use crate::toolkit::mind_toolbox::reasoning::ReasoningError;
use crate::toolkit::mind_toolbox::search::SearchError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddMemoryResult {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub deleted: Vec<String>,
    /// #44: existing memory_ids a duplicate write was deduped to (NOOP). Lets the
    /// agent tell "saved new" from "already known, linked" — not a silent skip.
    #[serde(default)]
    pub deduped: Vec<String>,
    pub skipped: usize,
    pub entities_extracted: usize,
    pub reasoning_relations_created: usize,
    pub chunks_created: usize,
    pub metadata: HashMap<String, serde_json::Value>,
    /// Charter escalations (memory-charter.md): conflicts Helixir is not
    /// allowed to resolve silently. Flag-don't-block: the decision already
    /// executed; the agent decides whether to ask the human.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs_clarification: Vec<Clarification>,
}

/// One write-path conflict surfaced to the agent per the memory charter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clarification {
    /// Charter conflict type: contradiction / cross_user_contradiction /
    /// low_confidence_rewrite / auto_delete.
    pub conflict_type: String,
    pub new_content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_memory_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_content: Option<String>,
    /// Question the agent can ask the user verbatim.
    pub suggested_question: String,
    /// What the engine decided (and already did) on its own.
    pub decision_taken: String,
    pub confidence: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMemoryResult {
    pub memory_id: String,
    pub content: String,
    pub score: f64,
    pub method: String,
    pub metadata: HashMap<String, serde_json::Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningChainSearchResult {
    pub chains: Vec<ToolingReasoningChain>,
    pub total_memories: usize,
    pub deepest_chain: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolingReasoningChain {
    pub seed: SearchMemoryResult,
    pub nodes: Vec<ChainNode>,
    pub chain_type: String,
    pub reasoning_trail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainNode {
    pub memory_id: String,
    pub content: String,
    pub relation: String,
    pub depth: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolingError {
    #[error("Embedding failed: {0}")]
    Embedding(String),
    #[error("Extraction failed: {0}")]
    Extraction(String),
    #[error("Chunking failed: {0}")]
    Chunking(#[from] ChunkingError),
    #[error("Entity operation failed: {0}")]
    Entity(#[from] EntityError),
    #[error("Ontology operation failed: {0}")]
    Ontology(#[from] OntologyError),
    #[error("Reasoning operation failed: {0}")]
    Reasoning(#[from] ReasoningError),
    #[error("Memory operation failed: {0}")]
    Memory(String),
    #[error("Search failed: {0}")]
    Search(#[from] SearchError),
    #[error("Database error: {0}")]
    Database(String),
}
