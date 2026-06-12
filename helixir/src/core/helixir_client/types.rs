//! Public DTOs returned from `HelixirClient` methods.
//!
//! These types are part of the client's public API: every `pub async fn` on
//! [`super::HelixirClient`] returns one of them. They are intentionally
//! decoupled from the internal [`crate::toolkit::tooling_manager`] result
//! types so the facade can evolve without breaking consumers.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddMemoryResult {
    pub memories_added: usize,
    pub memory_ids: Vec<String>,
    pub chunks_created: usize,
    pub entities_extracted: usize,
    pub relations_created: usize,
    pub stats: HashMap<String, serde_json::Value>,
    /// Charter escalations: conflicts the write path was not allowed to
    /// resolve silently (memory-charter.md). The agent decides whether to
    /// ask the human or apply a learned rule.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs_clarification: Vec<crate::toolkit::tooling_manager::types::Clarification>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub content: String,
    pub score: f32,
    pub metadata: HashMap<String, serde_json::Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateResult {
    pub memory_id: String,
    pub updated: bool,
    pub new_content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphResult {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningChainResult {
    pub query: String,
    pub chains: Vec<ReasoningChain>,
    pub total_memories: usize,
    pub deepest_chain: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningChain {
    pub seed: SearchResult,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub content: String,
    pub node_type: String,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub edge_type: String,
    pub weight: f32,
}

/// Result of `connect_memories` — the path between two anchors, if found.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectMemoriesResult {
    pub found: bool,
    pub hops: usize,
    /// Product of edge weights along the path (rough chain trust).
    pub confidence: f64,
    pub nodes: Vec<ConnectionNode>,
    pub edges: Vec<ConnectionEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionNode {
    pub memory_id: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionEdge {
    pub edge_type: String,
    pub weight: f64,
}
