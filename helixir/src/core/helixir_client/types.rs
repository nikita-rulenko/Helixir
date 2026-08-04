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
    /// Existing memory ids whose content was changed by the write decision.
    /// Empty for fresh adds, dedup no-ops, and decisions that do not update.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub updated: Vec<String>,
    /// #44: existing memory_ids this write deduped to (already known, not newly
    /// stored). Empty when everything was a fresh add. Lets the agent distinguish
    /// saved-new from linked-to-existing instead of seeing an empty memory_ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deduped: Vec<String>,
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

impl From<crate::toolkit::tooling_manager::AddMemoryResult> for AddMemoryResult {
    fn from(result: crate::toolkit::tooling_manager::AddMemoryResult) -> Self {
        Self {
            memories_added: result.added.len(),
            memory_ids: result.added,
            updated: result.updated,
            deduped: result.deduped,
            chunks_created: result.chunks_created,
            entities_extracted: result.entities_extracted,
            relations_created: result.reasoning_relations_created,
            stats: result.metadata,
            needs_clarification: result.needs_clarification,
        }
    }
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
    /// Both anchors resolved to the same memory — a shared relevant fact,
    /// not a discovered multi-hop path.
    #[serde(default)]
    pub shared_seed: bool,
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

#[cfg(test)]
mod tests {
    use super::AddMemoryResult;

    #[test]
    fn add_result_projection_preserves_updated_ids() {
        let internal = crate::toolkit::tooling_manager::AddMemoryResult {
            added: vec![],
            updated: vec!["mem_updated".to_string()],
            deleted: vec![],
            deduped: vec![],
            skipped: 0,
            entities_extracted: 0,
            reasoning_relations_created: 0,
            chunks_created: 0,
            metadata: Default::default(),
            needs_clarification: vec![],
        };

        let public = AddMemoryResult::from(internal);

        assert_eq!(public.updated, ["mem_updated"]);
        assert_eq!(
            serde_json::to_value(public).expect("serialize add result")["updated"],
            serde_json::json!(["mem_updated"])
        );
    }
}
