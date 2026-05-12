//! Pure data types of the reasoning subsystem plus the `project_relation`
//! projection helper used by chain traversal.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReasoningType {
    Implies,

    Because,

    Contradicts,

    Supports,
}

impl ReasoningType {
    #[must_use]
    pub fn edge_name(&self) -> &'static str {
        match self {
            Self::Implies => "IMPLIES",
            Self::Because => "BECAUSE",
            Self::Contradicts => "CONTRADICTS",
            Self::Supports => "SUPPORTS",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningRelation {
    pub relation_id: String,

    pub from_memory_id: String,

    pub to_memory_id: String,

    /// Content of `to_memory_id`. INVARIANT: when populated, this MUST match
    /// the content of the memory pointed to by `to_memory_id` — both incoming
    /// and outgoing edges discovered during `get_chain` traversal. See #17.
    pub to_memory_content: String,

    /// Content of `from_memory_id`. Defaults to empty string when unknown
    /// (e.g. on a freshly persisted relation that never went through `get_chain`).
    /// See #17.
    #[serde(default)]
    pub from_memory_content: String,

    pub relation_type: ReasoningType,

    pub strength: i32,

    pub reasoning_id: Option<String>,
}

/// Pure projection helper: given a current node and the freshly discovered
/// neighbour, produce a `ReasoningRelation` whose `(from_memory_id, to_memory_id)`
/// physically reflects the edge direction in storage AND whose
/// `(*_memory_id, *_memory_content)` pairs stay aligned. This is the
/// canonical fix for #17.
#[must_use]
pub(super) fn project_relation(
    current_id: &str,
    current_content: &str,
    neighbor_id: &str,
    neighbor_content: &str,
    relation_type: ReasoningType,
    is_incoming: bool,
    strength: i32,
) -> ReasoningRelation {
    let (from_id, from_content, to_id, to_content) = if is_incoming {
        (neighbor_id, neighbor_content, current_id, current_content)
    } else {
        (current_id, current_content, neighbor_id, neighbor_content)
    };

    ReasoningRelation {
        relation_id: format!(
            "rel_{}_{}",
            crate::safe_truncate(from_id, 8),
            crate::safe_truncate(to_id, 8)
        ),
        from_memory_id: from_id.to_string(),
        to_memory_id: to_id.to_string(),
        to_memory_content: to_content.to_string(),
        from_memory_content: from_content.to_string(),
        relation_type,
        strength,
        reasoning_id: None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningChain {
    pub seed_memory_id: String,

    pub relations: Vec<ReasoningRelation>,

    pub chain_type: String,

    pub depth: usize,

    pub reasoning_trail: String,
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub size: usize,

    pub capacity: usize,

    pub is_warmed_up: bool,
}

#[derive(Debug, Error)]
pub enum ReasoningError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Invalid relation: {0}")]
    Invalid(String),

    #[error("LLM error: {0}")]
    LlmError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reasoning_type_edge_name() {
        assert_eq!(ReasoningType::Implies.edge_name(), "IMPLIES");
        assert_eq!(ReasoningType::Because.edge_name(), "BECAUSE");
        assert_eq!(ReasoningType::Contradicts.edge_name(), "CONTRADICTS");
        assert_eq!(ReasoningType::Supports.edge_name(), "SUPPORTS");
    }

    #[test]
    fn test_relation_creation() {
        let relation = ReasoningRelation {
            relation_id: "test".to_string(),
            from_memory_id: "mem_1".to_string(),
            to_memory_id: "mem_2".to_string(),
            to_memory_content: "test content".to_string(),
            from_memory_content: String::new(),
            relation_type: ReasoningType::Implies,
            strength: 80,
            reasoning_id: None,
        };

        assert_eq!(relation.strength, 80);
        assert_eq!(relation.relation_type, ReasoningType::Implies);
    }

    #[test]
    fn project_relation_outgoing_pairs_id_with_content() {
        // Outgoing edge (current → neighbor): #17 invariant.
        let rel = project_relation(
            "mem_current",
            "current content",
            "mem_neighbor",
            "neighbor content",
            ReasoningType::Implies,
            false,
            80,
        );

        assert_eq!(rel.from_memory_id, "mem_current");
        assert_eq!(rel.from_memory_content, "current content");
        assert_eq!(rel.to_memory_id, "mem_neighbor");
        assert_eq!(rel.to_memory_content, "neighbor content");
    }

    #[test]
    fn project_relation_incoming_pairs_id_with_content() {
        // Incoming edge (neighbor → current): #17 specifically targeted this case
        // where `to_memory_content` used to leak the neighbour's content.
        let rel = project_relation(
            "mem_current",
            "current content",
            "mem_neighbor",
            "neighbor content",
            ReasoningType::Because,
            true,
            80,
        );

        assert_eq!(rel.from_memory_id, "mem_neighbor");
        assert_eq!(rel.from_memory_content, "neighbor content");
        assert_eq!(rel.to_memory_id, "mem_current");
        assert_eq!(rel.to_memory_content, "current content");
    }
}
