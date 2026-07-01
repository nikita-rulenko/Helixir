//! Pure data types of the reasoning subsystem plus the `project_relation`
//! projection helper used by chain traversal.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A typed memory→memory edge. The first four are CAUSAL/logical (they form
/// reasoning chains and are what `search_reasoning_chain` walks); the rest are
/// ASSOCIATIVE/structural (relatedness, composition, taxonomy) — they enrich
/// the graph and surface in `get_memory_graph`, but do not claim causality.
/// All persist uniformly as a `MEMORY_RELATION` edge whose `relation_type`
/// property is the `edge_name()` string, so adding a variant needs no schema
/// change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReasoningType {
    // --- causal / logical ---
    Implies,
    Because,
    Contradicts,
    Supports,
    // --- associative / structural ---
    /// General relatedness — same topic / near-duplicate, no causal claim.
    RelatesTo,
    /// Compositional: A is a part/component of B.
    PartOf,
    /// Taxonomic: A is a kind/instance of B.
    IsA,
}

impl ReasoningType {
    #[must_use]
    pub fn edge_name(&self) -> &'static str {
        match self {
            Self::Implies => "IMPLIES",
            Self::Because => "BECAUSE",
            Self::Contradicts => "CONTRADICTS",
            Self::Supports => "SUPPORTS",
            Self::RelatesTo => "RELATES_TO",
            Self::PartOf => "PART_OF",
            Self::IsA => "IS_A",
        }
    }

    /// Parse an LLM-supplied edge token. Unknown tokens fall back to the safe
    /// generic `RelatesTo` — never to a causal type, so a misread never invents
    /// a false cause/effect claim (the old code silently coerced to IMPLIES).
    #[must_use]
    pub fn from_token(s: &str) -> Self {
        match s.trim().to_uppercase().as_str() {
            "IMPLIES" => Self::Implies,
            "BECAUSE" => Self::Because,
            "CONTRADICTS" => Self::Contradicts,
            "SUPPORTS" => Self::Supports,
            "PART_OF" | "PARTOF" => Self::PartOf,
            "IS_A" | "ISA" | "INSTANCE_OF" => Self::IsA,
            // "RELATES_TO" and anything unrecognised → generic relatedness.
            _ => Self::RelatesTo,
        }
    }

    /// Causal/logical edges form reasoning chains; associative ones do not.
    #[must_use]
    pub fn is_causal(&self) -> bool {
        matches!(
            self,
            Self::Implies | Self::Because | Self::Contradicts | Self::Supports
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningRelation {
    pub relation_id: String,

    /// The node on the other end of this hop, relative to the chain's
    /// current position — what an agent wants to SEE in nodes[] (GH#23).
    /// Direction stays available via from/to.
    #[serde(default)]
    pub peer_memory_id: String,
    #[serde(default)]
    pub peer_memory_content: String,

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
        peer_memory_id: neighbor_id.to_string(),
        peer_memory_content: neighbor_content.to_string(),
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
        // Associative arsenal (P0: these must actually build, not collapse).
        assert_eq!(ReasoningType::RelatesTo.edge_name(), "RELATES_TO");
        assert_eq!(ReasoningType::PartOf.edge_name(), "PART_OF");
        assert_eq!(ReasoningType::IsA.edge_name(), "IS_A");
    }

    #[test]
    fn from_token_covers_full_arsenal_and_is_safe() {
        // Every edge token round-trips through from_token → edge_name.
        for t in [
            ReasoningType::Implies,
            ReasoningType::Because,
            ReasoningType::Contradicts,
            ReasoningType::Supports,
            ReasoningType::RelatesTo,
            ReasoningType::PartOf,
            ReasoningType::IsA,
        ] {
            assert_eq!(ReasoningType::from_token(t.edge_name()), t);
        }
        // Case / whitespace tolerant + synonyms.
        assert_eq!(
            ReasoningType::from_token(" implies "),
            ReasoningType::Implies
        );
        assert_eq!(ReasoningType::from_token("instance_of"), ReasoningType::IsA);
        // CRITICAL: an unknown token must fall back to the generic RELATES_TO,
        // NEVER to a false causal IMPLIES (the old silent-coercion bug).
        assert_eq!(
            ReasoningType::from_token("ELABORATES"),
            ReasoningType::RelatesTo
        );
        assert_eq!(ReasoningType::from_token(""), ReasoningType::RelatesTo);
    }

    #[test]
    fn is_causal_splits_reasoning_from_association() {
        assert!(ReasoningType::Because.is_causal());
        assert!(ReasoningType::Implies.is_causal());
        assert!(ReasoningType::Supports.is_causal());
        assert!(ReasoningType::Contradicts.is_causal());
        assert!(!ReasoningType::RelatesTo.is_causal());
        assert!(!ReasoningType::PartOf.is_causal());
        assert!(!ReasoningType::IsA.is_causal());
    }

    #[test]
    fn test_relation_creation() {
        let relation = ReasoningRelation {
            peer_memory_id: String::new(),
            peer_memory_content: String::new(),
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
