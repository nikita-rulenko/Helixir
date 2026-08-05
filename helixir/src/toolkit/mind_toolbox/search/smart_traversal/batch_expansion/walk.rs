//! Scan-free levelwise graph walk over HelixDB primary keys.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tracing::{debug, info};

use super::*;

pub async fn graph_expansion_phase_batched(
    client: Arc<HelixClient>,
    vector_hits: &[SearchResult],
    config: &SearchConfig,
) -> Result<ExpansionOutput, TraversalError> {
    let max_depth = config.graph_depth;
    info!(
        "Starting Phase 2 (batched): levelwise expansion from {} seeds, depth {}",
        vector_hits.len(),
        max_depth
    );

    let mut results: Vec<SearchResult> = Vec::new();
    let mut ego_edges: Vec<PprEdge> = Vec::new();
    let mut seen_edges: HashSet<(String, String, &'static str)> = HashSet::new();
    let mut visited: HashSet<String> = vector_hits.iter().map(|h| h.memory_id.clone()).collect();
    let mut frontier: HashMap<String, (String, f64)> = vector_hits
        .iter()
        .filter_map(|hit| {
            hit.internal_id
                .as_ref()
                .map(|id| (hit.memory_id.clone(), (id.clone(), hit.combined_score)))
        })
        .collect();

    for depth in 1..=max_depth {
        if frontier.is_empty() {
            break;
        }

        let internal_ids: Vec<&str> = frontier
            .values()
            .map(|(internal_id, _)| internal_id.as_str())
            .collect();
        let response = fetch_level_by_internal_ids(&client, &internal_ids).await?;

        let mut node_by_uuid: HashMap<&str, &BatchNode> = HashMap::new();
        let mut parent_score_by_uuid: HashMap<&str, f64> = HashMap::new();
        for memory in &response.memories {
            node_by_uuid.insert(memory.id.as_str(), memory);
            if let Some((_, score)) = frontier.get(&memory.memory_id) {
                parent_score_by_uuid.insert(memory.id.as_str(), *score);
            }
        }
        let fams = families(&response, config.edge_weights, config.edge_damping);
        for (_, nodes, _, _, _) in &fams {
            for node in *nodes {
                node_by_uuid.insert(node.id.as_str(), node);
            }
        }

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

                let effective_weight = *edge_weight * edge.strength_norm();
                if let Some(parent_node) = node_by_uuid.get(parent_uuid) {
                    let key = (
                        parent_node.memory_id.clone(),
                        child.memory_id.clone(),
                        *edge_type,
                    );
                    if seen_edges.insert(key) {
                        ego_edges.push(PprEdge {
                            from: parent_node.memory_id.clone(),
                            to: child.memory_id.clone(),
                            weight: effective_weight,
                        });
                    }
                }

                if visited.contains(&child.memory_id) {
                    continue;
                }

                let graph_score = calculate_graph_score(effective_weight, *parent_score);
                let temporal_score = calculate_temporal_freshness(
                    super::super::scoring::event_time(&child.valid_from, &child.created_at),
                    config.temporal_decay_days,
                );
                let mut result = SearchResult::from_graph_weighted(
                    &child.memory_id,
                    &child.content,
                    GraphScores {
                        semantic_sim: 0.5,
                        graph_score,
                        temporal_score,
                    },
                    depth,
                    vec![edge_type.to_string()],
                    ScoreWeights {
                        semantic: config.graph_semantic_weight,
                        graph: config.graph_graph_weight,
                        temporal: config.graph_temporal_weight,
                    },
                );
                result.internal_id = Some(child.id.clone());
                result.created_at = Some(child.created_at.clone());
                result.valid_from = Some(child.valid_from.clone());

                let parent_memory_id = node_by_uuid
                    .get(parent_uuid)
                    .map(|parent| parent.memory_id.clone())
                    .unwrap_or_default();
                let mut metadata = HashMap::new();
                metadata.insert(
                    "origin".to_string(),
                    serde_json::Value::String("graph".to_string()),
                );
                metadata.insert(
                    "edge".to_string(),
                    serde_json::Value::String(edge_type.to_string()),
                );
                metadata.insert(
                    "parent".to_string(),
                    serde_json::Value::String(parent_memory_id),
                );
                metadata.insert("depth".to_string(), serde_json::Value::from(depth));
                if !child.user_id.is_empty() {
                    metadata.insert(
                        "user_id".to_string(),
                        serde_json::Value::String(child.user_id.clone()),
                    );
                }
                if !child.memory_type.is_empty() {
                    metadata.insert(
                        "memory_type".to_string(),
                        serde_json::Value::String(child.memory_type.clone()),
                    );
                }
                if !child.content_key.is_empty() {
                    metadata.insert(
                        "content_key".to_string(),
                        serde_json::Value::String(child.content_key.clone()),
                    );
                }
                result.metadata = Some(metadata);
                results.push(result);
                children_by_parent.entry(parent_uuid).or_default().push((
                    child,
                    graph_score,
                    edge_type,
                ));
            }
        }

        let mut next_frontier: HashMap<String, (String, f64)> = HashMap::new();
        for (_, mut children) in children_by_parent {
            children.sort_by(|left, right| {
                right
                    .1
                    .partial_cmp(&left.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for (child, graph_score, _) in children.into_iter().take(config.beam_width.max(1)) {
                if visited.insert(child.memory_id.clone()) {
                    let entry = next_frontier
                        .entry(child.memory_id.clone())
                        .or_insert_with(|| (child.id.clone(), graph_score));
                    if graph_score > entry.1 {
                        entry.1 = graph_score;
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
        "Phase 2 (batched) completed: {} expanded results, {} ego edges",
        results.len(),
        ego_edges.len()
    );
    Ok(ExpansionOutput {
        results,
        edges: ego_edges,
    })
}
