//! Levelwise batched graph expansion (algo_opt, research doc §6 P1.3).
//!
//! Replaces the per-node recursive DFS of [`super::phases::graph_expansion_phase`]
//! with a breadth-first walk that fetches the whole frontier's neighbourhood in
//! **one** `getConnectionsLevelBatch` HQL call per depth level. Round-trips drop
//! from O(visited nodes) to O(depth).
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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Deserialize;
use tracing::{debug, info};

use super::models::{SearchConfig, SearchResult, edge_weights};
use super::phases::TraversalError;
use super::scoring::{calculate_graph_score, calculate_temporal_freshness};
use crate::db::HelixClient;

#[derive(Debug, Deserialize, Clone)]
struct BatchNode {
    id: String,
    memory_id: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct BatchEdge {
    from_node: String,
    to_node: String,
}

#[derive(Debug, Deserialize)]
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

/// `(edges, neighbour nodes, edge label, weight, incoming?)` per family.
fn families(
    r: &LevelBatchResponse,
) -> [(&[BatchEdge], &[BatchNode], &'static str, f64, bool); 8] {
    [
        (
            &r.implies_out_e,
            &r.implies_out_n,
            "IMPLIES",
            edge_weights::IMPLIES,
            false,
        ),
        (
            &r.because_out_e,
            &r.because_out_n,
            "BECAUSE",
            edge_weights::BECAUSE,
            false,
        ),
        (
            &r.contradicts_out_e,
            &r.contradicts_out_n,
            "CONTRADICTS",
            edge_weights::CONTRADICTS,
            false,
        ),
        (
            &r.relation_out_e,
            &r.relation_out_n,
            "MEMORY_RELATION",
            edge_weights::MEMORY_RELATION,
            false,
        ),
        (
            &r.implies_in_e,
            &r.implies_in_n,
            "IMPLIES_IN",
            edge_weights::IMPLIES * 0.9,
            true,
        ),
        (
            &r.because_in_e,
            &r.because_in_n,
            "BECAUSE_IN",
            edge_weights::BECAUSE * 0.85,
            true,
        ),
        (
            &r.contradicts_in_e,
            &r.contradicts_in_n,
            "CONTRADICTS_IN",
            edge_weights::CONTRADICTS * 0.8,
            true,
        ),
        (
            &r.relation_in_e,
            &r.relation_in_n,
            "MEMORY_RELATION_IN",
            edge_weights::MEMORY_RELATION * 0.6,
            true,
        ),
    ]
}

pub async fn graph_expansion_phase_batched(
    client: Arc<HelixClient>,
    vector_hits: &[SearchResult],
    config: &SearchConfig,
) -> Result<Vec<SearchResult>, TraversalError> {
    let max_depth = config.graph_depth;
    info!(
        "Starting Phase 2 (batched): levelwise expansion from {} seeds, depth {}",
        vector_hits.len(),
        max_depth
    );

    let mut results: Vec<SearchResult> = Vec::new();
    let mut visited: HashSet<String> = vector_hits
        .iter()
        .map(|h| h.memory_id.clone())
        .collect();
    // memory_id -> score the children inherit (combined for seeds, graph for deeper).
    let mut frontier: HashMap<String, f64> = vector_hits
        .iter()
        .map(|h| (h.memory_id.clone(), h.combined_score))
        .collect();

    for depth in 1..=max_depth {
        if frontier.is_empty() {
            break;
        }

        let ids: Vec<&str> = frontier.keys().map(String::as_str).collect();
        let params = serde_json::json!({ "memory_ids": ids });
        let response: LevelBatchResponse = client
            .execute_query("getConnectionsLevelBatch", &params)
            .await
            .map_err(|e| TraversalError::Database(e.to_string()))?;

        // uuid -> node for every node that came back on this level.
        let mut node_by_uuid: HashMap<&str, &BatchNode> = HashMap::new();
        // uuid -> inherited score for the anchors of this level.
        let mut parent_score_by_uuid: HashMap<&str, f64> = HashMap::new();
        for m in &response.memories {
            node_by_uuid.insert(m.id.as_str(), m);
            if let Some(score) = frontier.get(&m.memory_id) {
                parent_score_by_uuid.insert(m.id.as_str(), *score);
            }
        }
        let fams = families(&response);
        for (_, nodes, _, _, _) in &fams {
            for n in *nodes {
                node_by_uuid.insert(n.id.as_str(), n);
            }
        }

        // parent uuid -> candidate children of this level.
        let mut children_by_parent: HashMap<&str, Vec<(&BatchNode, f64, &'static str)>> =
            HashMap::new();

        for (edges, _, edge_type, edge_weight, incoming) in &fams {
            for edge in *edges {
                let (parent_uuid, child_uuid) = if *incoming {
                    (edge.to_node.as_str(), edge.from_node.as_str())
                } else {
                    (edge.from_node.as_str(), edge.to_node.as_str())
                };
                let Some(parent_score) = parent_score_by_uuid.get(parent_uuid) else {
                    continue;
                };
                let Some(child) = node_by_uuid.get(child_uuid) else {
                    continue;
                };
                if visited.contains(&child.memory_id) {
                    continue;
                }

                let graph_score = calculate_graph_score(*edge_weight, *parent_score);
                let temporal_score =
                    calculate_temporal_freshness(&child.created_at, config.temporal_decay_days);

                // Same as the legacy DFS: every unvisited neighbour becomes a
                // result; the 0.5 placeholder is replaced by the P0.2 re-rank.
                results.push(SearchResult::from_graph_weighted(
                    &child.memory_id,
                    &child.content,
                    0.5,
                    graph_score,
                    temporal_score,
                    1,
                    vec![edge_type.to_string()],
                    config.graph_semantic_weight,
                    config.graph_graph_weight,
                    config.graph_temporal_weight,
                ));

                children_by_parent
                    .entry(parent_uuid)
                    .or_default()
                    .push((child, graph_score, edge_type));
            }
        }

        // Top-3 children per parent move to the next frontier (legacy take(3)).
        let mut next_frontier: HashMap<String, f64> = HashMap::new();
        for (_, mut children) in children_by_parent {
            children.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            for (child, graph_score, _) in children.into_iter().take(3) {
                if visited.insert(child.memory_id.clone()) {
                    let entry = next_frontier
                        .entry(child.memory_id.clone())
                        .or_insert(graph_score);
                    if graph_score > *entry {
                        *entry = graph_score;
                    }
                }
            }
        }

        debug!(
            "Batched expansion level {}: {} anchors, {} results so far, {} next frontier",
            depth,
            frontier.len(),
            results.len(),
            next_frontier.len()
        );
        frontier = next_frontier;
    }

    info!(
        "Phase 2 (batched) completed: {} expanded results",
        results.len()
    );
    Ok(results)
}
