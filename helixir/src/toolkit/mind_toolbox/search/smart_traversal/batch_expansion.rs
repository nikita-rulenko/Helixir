//! Levelwise batched graph expansion (algo_opt, research doc §6 P1.3).
//!
//! Replaces the per-node recursive DFS of [`super::phases::graph_expansion_phase`]
//! with a breadth-first walk that fetches each frontier node by its HelixDB
//! primary key. This intentionally trades a bounded number of local round-trips
//! for eliminating the `N<Memory>::WHERE(IS_IN(...))` label scans whose v2.3.5
//! request arenas grow monotonically (#89).
//!
//! Semantics mirror the legacy expansion:
//! - every unvisited neighbour becomes a `SearchResult` (deduped later by
//!   `rank_and_filter` on max combined score);
//! - only the top-3 children **per parent** (by graph score) join the next
//!   frontier;
//! - the same per-family edge weights apply, including the dampened `*_IN`
//!   variants;
//! - `semantic_sim` starts at the legacy 0.5 placeholder — under `algo_opt`
//!   the caller re-scores graph results with real cosine right after this
//!   phase (P0.2), exactly as it does for the DFS path.

use std::collections::HashMap;

use serde::Deserialize;

use super::models::{GraphScores, ScoreWeights, SearchConfig, SearchResult};
use super::phases::TraversalError;
use super::ppr::PprEdge;
use super::scoring::{calculate_graph_score, calculate_temporal_freshness};
use crate::db::HelixClient;

mod walk;
pub use walk::graph_expansion_phase_batched;

/// Expansion results plus the ego-network edges collected on the way —
/// the input for PPR re-ranking (elder-brain #9).
pub struct ExpansionOutput {
    pub results: Vec<SearchResult>,
    pub edges: Vec<PprEdge>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct BatchNode {
    pub(crate) id: String,
    pub(crate) memory_id: String,
    #[serde(default)]
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) created_at: String,
    #[serde(default)]
    pub(crate) valid_from: String,
    #[serde(default)]
    pub(crate) user_id: String,
    #[serde(default)]
    pub(crate) memory_type: String,
    #[serde(default)]
    pub(crate) content_key: String,
}

#[derive(Debug, Deserialize)]
struct BatchEdge {
    from_node: String,
    to_node: String,
    // Per-edge confidence the writer (LLM) assigned. IMPLIES carries it as
    // `probability`, the others as `strength`. Optional: older edges lack it.
    #[serde(default)]
    strength: Option<i64>,
    #[serde(default)]
    probability: Option<i64>,
}

impl BatchEdge {
    /// The writer's per-edge confidence normalised to `0..1` (`strength` or
    /// `probability` ÷ 100); `1.0` when the edge stored none, so an unweighted
    /// (legacy) edge is a no-op multiplier.
    fn strength_norm(&self) -> f64 {
        self.strength
            .or(self.probability)
            .map(|s| (s as f64 / 100.0).clamp(0.0, 1.0))
            .unwrap_or(1.0)
    }
}

#[derive(Debug, Deserialize, Default)]
struct LevelBatchResponse {
    #[serde(default)]
    memories: Vec<BatchNode>,
    #[serde(default)]
    implies_out_e: Vec<BatchEdge>,
    #[serde(default)]
    implies_out_n: Vec<BatchNode>,
    #[serde(default)]
    implies_in_e: Vec<BatchEdge>,
    #[serde(default)]
    implies_in_n: Vec<BatchNode>,
    #[serde(default)]
    because_out_e: Vec<BatchEdge>,
    #[serde(default)]
    because_out_n: Vec<BatchNode>,
    #[serde(default)]
    because_in_e: Vec<BatchEdge>,
    #[serde(default)]
    because_in_n: Vec<BatchNode>,
    #[serde(default)]
    contradicts_out_e: Vec<BatchEdge>,
    #[serde(default)]
    contradicts_out_n: Vec<BatchNode>,
    #[serde(default)]
    contradicts_in_e: Vec<BatchEdge>,
    #[serde(default)]
    contradicts_in_n: Vec<BatchNode>,
    #[serde(default)]
    relation_out_e: Vec<BatchEdge>,
    #[serde(default)]
    relation_out_n: Vec<BatchNode>,
    #[serde(default)]
    relation_in_e: Vec<BatchEdge>,
    #[serde(default)]
    relation_in_n: Vec<BatchNode>,
}

#[derive(Debug, Deserialize, Default)]
struct LevelSingleResponse {
    #[serde(default)]
    memory: Option<BatchNode>,
    #[serde(default)]
    implies_out_e: Vec<BatchEdge>,
    #[serde(default)]
    implies_out_n: Vec<BatchNode>,
    #[serde(default)]
    implies_in_e: Vec<BatchEdge>,
    #[serde(default)]
    implies_in_n: Vec<BatchNode>,
    #[serde(default)]
    because_out_e: Vec<BatchEdge>,
    #[serde(default)]
    because_out_n: Vec<BatchNode>,
    #[serde(default)]
    because_in_e: Vec<BatchEdge>,
    #[serde(default)]
    because_in_n: Vec<BatchNode>,
    #[serde(default)]
    contradicts_out_e: Vec<BatchEdge>,
    #[serde(default)]
    contradicts_out_n: Vec<BatchNode>,
    #[serde(default)]
    contradicts_in_e: Vec<BatchEdge>,
    #[serde(default)]
    contradicts_in_n: Vec<BatchNode>,
    #[serde(default)]
    relation_out_e: Vec<BatchEdge>,
    #[serde(default)]
    relation_out_n: Vec<BatchNode>,
    #[serde(default)]
    relation_in_e: Vec<BatchEdge>,
    #[serde(default)]
    relation_in_n: Vec<BatchNode>,
}

impl LevelBatchResponse {
    fn append_single(&mut self, mut single: LevelSingleResponse) {
        if let Some(memory) = single.memory.take() {
            self.memories.push(memory);
        }
        self.implies_out_e.append(&mut single.implies_out_e);
        self.implies_out_n.append(&mut single.implies_out_n);
        self.implies_in_e.append(&mut single.implies_in_e);
        self.implies_in_n.append(&mut single.implies_in_n);
        self.because_out_e.append(&mut single.because_out_e);
        self.because_out_n.append(&mut single.because_out_n);
        self.because_in_e.append(&mut single.because_in_e);
        self.because_in_n.append(&mut single.because_in_n);
        self.contradicts_out_e.append(&mut single.contradicts_out_e);
        self.contradicts_out_n.append(&mut single.contradicts_out_n);
        self.contradicts_in_e.append(&mut single.contradicts_in_e);
        self.contradicts_in_n.append(&mut single.contradicts_in_n);
        self.relation_out_e.append(&mut single.relation_out_e);
        self.relation_out_n.append(&mut single.relation_out_n);
        self.relation_in_e.append(&mut single.relation_in_e);
        self.relation_in_n.append(&mut single.relation_in_n);
    }
}

async fn fetch_level_by_internal_ids(
    client: &HelixClient,
    internal_ids: &[&str],
) -> Result<LevelBatchResponse, TraversalError> {
    let mut merged = LevelBatchResponse::default();
    for internal_id in internal_ids {
        let response: LevelSingleResponse = client
            .execute_query(
                "getConnectionsByInternalId",
                &serde_json::json!({ "internal_id": internal_id }),
            )
            .await
            .map_err(|error| TraversalError::Database(error.to_string()))?;
        merged.append_single(response);
    }
    Ok(merged)
}

type EdgeFamily<'a> = (&'a [BatchEdge], &'a [BatchNode], &'static str, f64, bool);

/// `(edges, neighbour nodes, edge label, weight, incoming?)` per family.
/// Weights + incoming dampeners come from config (passed in by the caller).
fn families(
    r: &LevelBatchResponse,
    ew: crate::core::config::EdgeWeights,
    ed: crate::core::config::EdgeDamping,
) -> [EdgeFamily<'_>; 8] {
    [
        (
            &r.implies_out_e,
            &r.implies_out_n,
            "IMPLIES",
            ew.implies,
            false,
        ),
        (
            &r.because_out_e,
            &r.because_out_n,
            "BECAUSE",
            ew.because,
            false,
        ),
        (
            &r.contradicts_out_e,
            &r.contradicts_out_n,
            "CONTRADICTS",
            ew.contradicts,
            false,
        ),
        (
            &r.relation_out_e,
            &r.relation_out_n,
            "MEMORY_RELATION",
            ew.memory_relation,
            false,
        ),
        (
            &r.implies_in_e,
            &r.implies_in_n,
            "IMPLIES_IN",
            ew.implies * ed.implies_in,
            true,
        ),
        (
            &r.because_in_e,
            &r.because_in_n,
            "BECAUSE_IN",
            ew.because * ed.because_in,
            true,
        ),
        (
            &r.contradicts_in_e,
            &r.contradicts_in_n,
            "CONTRADICTS_IN",
            ew.contradicts * ed.contradicts_in,
            true,
        ),
        (
            &r.relation_in_e,
            &r.relation_in_n,
            "MEMORY_RELATION_IN",
            ew.memory_relation * ed.relation_in,
            true,
        ),
    ]
}

/// One direction-resolved edge of a fetched level: `parent` anchors the
/// frontier, `child` is the node on the other end.
pub(crate) struct LevelEdge {
    pub(crate) parent_uuid: String,
    pub(crate) child_uuid: String,
    pub(crate) edge_type: &'static str,
    /// Per-family structural weight (direction/type semantics, dampened `*_IN`).
    pub(crate) weight: f64,
    /// The writer's per-edge confidence normalised to `0..1` (LLM `strength` /
    /// `probability` ÷ 100); `1.0` when the edge stored none. Distinct from
    /// `weight` so existing consumers keep family-weight semantics while
    /// longest-chain can fold in real per-edge confidence.
    pub(crate) strength_norm: f64,
}

pub(crate) struct LevelFetch {
    pub(crate) nodes_by_uuid: HashMap<String, BatchNode>,
    pub(crate) edges: Vec<LevelEdge>,
}

/// Fetches the whole frontier's neighbourhood in one HQL call and resolves
/// edge directions (shared by graph expansion and connect_memories).
pub(crate) async fn fetch_level(
    client: &HelixClient,
    memory_ids: &[&str],
    ew: crate::core::config::EdgeWeights,
    ed: crate::core::config::EdgeDamping,
) -> Result<LevelFetch, TraversalError> {
    let params = serde_json::json!({ "memory_ids": memory_ids });
    let response: LevelBatchResponse = client
        .execute_query("getConnectionsLevelBatch", &params)
        .await
        .map_err(|e| TraversalError::Database(e.to_string()))?;

    let mut nodes_by_uuid: HashMap<String, BatchNode> = HashMap::new();
    for m in &response.memories {
        nodes_by_uuid.insert(m.id.clone(), m.clone());
    }
    let fams = families(&response, ew, ed);
    for (_, nodes, _, _, _) in &fams {
        for n in *nodes {
            nodes_by_uuid.insert(n.id.clone(), n.clone());
        }
    }

    let mut edges = Vec::new();
    for (fam_edges, _, edge_type, weight, incoming) in &fams {
        for e in *fam_edges {
            let (parent_uuid, child_uuid) = if *incoming {
                (e.to_node.clone(), e.from_node.clone())
            } else {
                (e.from_node.clone(), e.to_node.clone())
            };
            edges.push(LevelEdge {
                parent_uuid,
                child_uuid,
                edge_type,
                weight: *weight,
                strength_norm: e.strength_norm(),
            });
        }
    }

    Ok(LevelFetch {
        nodes_by_uuid,
        edges,
    })
}
