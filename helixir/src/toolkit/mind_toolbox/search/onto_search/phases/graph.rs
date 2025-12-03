

use std::collections::HashSet;
use tracing::{debug, info};
use crate::db::HelixClient;
use super::super::config::OntoSearchConfig;
use super::super::models::{GraphContext, OntoSearchResult};
use super::super::temporal::calculate_temporal_freshness;


const EDGE_WEIGHTS: &[(&str, &str, f64)] = &[
    ("implies_out", "IMPLIES", 0.9),
    ("implies_in", "IMPLIES", 0.8),
    ("because_out", "BECAUSE", 0.95),
    ("because_in", "BECAUSE", 0.85),
    ("relation_out", "MEMORY_RELATION", 0.7),
    ("relation_in", "MEMORY_RELATION", 0.6),
];


pub async fn expand_from_memory(
    client: &HelixClient,
    memory_id: &str,
    visited: &mut HashSet<String>,
    config: &OntoSearchConfig,
) -> Vec<OntoSearchResult> {
    let params = serde_json::json!({"memory_id": memory_id});
    let result: serde_json::Value = match client.execute_query("getMemoryLogicalConnections", &params).await {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let mut expansion = Vec::new();

    for (field, edge_type, weight) in EDGE_WEIGHTS {
        let Some(memories) = result.get(*field).and_then(|v| v.as_array()) else { continue };

        for mem in memories {
            let Some(target_id) = mem.get("memory_id").and_then(|v| v.as_str()) else { continue };
            if visited.contains(target_id) { continue; }
            visited.insert(target_id.to_string());

            let created_at = mem.get("created_at").and_then(|v| v.as_str()).unwrap_or("");

            expansion.push(OntoSearchResult {
                memory_id: target_id.to_string(),
                content: mem.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                memory_type: mem.get("memory_type").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                user_id: mem.get("user_id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                vector_score: 0.5,
                graph_score: *weight,
                temporal_score: calculate_temporal_freshness(created_at, config.temporal_decay_rate),
                created_at: created_at.to_string(),
                depth: 1,
                source: "graph".to_string(),
                graph_context: Some(GraphContext {
                    related_memories: vec![memory_id.to_string()],
                    edge_types: vec![edge_type.to_string()],
                    edge_weights: vec![*weight],
                }),
                ..Default::default()
            });
        }
    }

    debug!("Expanded from {}: {} neighbors", crate::safe_truncate(memory_id, 8), expansion.len());
    expansion
}


pub async fn graph_expansion_phase(
    client: &HelixClient,
    results: &[OntoSearchResult],
    config: &OntoSearchConfig,
) -> Vec<OntoSearchResult> {
    if config.graph_depth == 0 {
        return results.to_vec();
    }

    let mut expanded = results.to_vec();
    let mut visited: HashSet<String> = results.iter().map(|r| r.memory_id.clone()).collect();

    for result in results {
        let neighbors = expand_from_memory(client, &result.memory_id, &mut visited, config).await;
        expanded.extend(neighbors);
    }

    info!("Graph expansion: {} → {} results", results.len(), expanded.len());
    expanded
}

