

use std::collections::HashSet;
use std::sync::Arc;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use super::models::{SearchResult, edge_weights};
use super::scoring::{calculate_temporal_freshness, calculate_graph_score};
use crate::db::HelixClient;


#[derive(Debug, thiserror::Error)]
pub enum TraversalError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Invalid query: {0}")]
    InvalidQuery(String),
}


#[derive(Debug, Deserialize, Default)]
struct VectorSearchResponse {
    #[serde(default)]
    memories: Vec<VectorMemory>,
    #[serde(default)]
    chunks: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize, Clone)]
struct VectorMemory {
    memory_id: String,
    content: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    memory_type: String,
    #[serde(default)]
    user_id: String,
}


#[derive(Debug, Deserialize, Default)]
struct GraphConnectionsResponse {
    #[serde(default)]
    implies_out: Vec<ConnectedMemory>,
    #[serde(default)]
    implies_in: Vec<ConnectedMemory>,
    #[serde(default)]
    because_out: Vec<ConnectedMemory>,
    #[serde(default)]
    because_in: Vec<ConnectedMemory>,
    #[serde(default)]
    contradicts_out: Vec<ConnectedMemory>,
    #[serde(default)]
    contradicts_in: Vec<ConnectedMemory>,
    #[serde(default)]
    relation_out: Vec<ConnectedMemory>,
    #[serde(default)]
    relation_in: Vec<ConnectedMemory>,
}


#[derive(Debug, Deserialize)]
struct ConnectedMemory {
    memory_id: String,
    content: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    memory_type: String,
}


pub async fn vector_search_phase(
    client: Arc<HelixClient>,
    query_embedding: &[f32],
    user_id: Option<&str>,
    top_k: usize,
    min_score: f64,
    temporal_cutoff: Option<DateTime<Utc>>,
) -> Result<Vec<SearchResult>, TraversalError> {
    info!("Starting Phase 1: Vector search with top_k={}", top_k);

    
    let query_vector: Vec<f64> = query_embedding.iter().map(|&x| x as f64).collect();
    let params = serde_json::json!({
        "query_vector": query_vector,
        "limit": top_k as i64
    });
    
    
    let _ = (user_id, min_score); 

    let response: VectorSearchResponse = client
        .execute_query("smartVectorSearchWithChunks", &params)
        .await
        .map_err(|e| TraversalError::Database(e.to_string()))?;

    let mut results = Vec::new();
    let mut seen_ids = HashSet::new();

    for memory in response.memories {
        if seen_ids.contains(&memory.memory_id) {
            continue;
        }
        seen_ids.insert(memory.memory_id.clone());

        if let Some(cutoff) = &temporal_cutoff {
            if let Ok(created_at) = DateTime::parse_from_rfc3339(&memory.created_at) {
                if created_at.with_timezone(&Utc) < *cutoff {
                    continue;
                }
            }
        }

        let temporal_score = calculate_temporal_freshness(&memory.created_at, 30.0);
        
        let mut result = SearchResult::from_vector(
            &memory.memory_id,
            &memory.content,
            0.8,
            temporal_score,
        );
        result.created_at = Some(memory.created_at.clone());

        if result.combined_score >= min_score {
            results.push(result);
        }
    }

    
    results.sort_by(|a, b| b.combined_score.partial_cmp(&a.combined_score).unwrap());

    info!("Phase 1 completed: {} results", results.len());
    Ok(results)
}


pub async fn graph_expansion_phase(
    client: Arc<HelixClient>,
    vector_hits: &[SearchResult],
    query_embedding: &[f32],
    max_depth: u32,
    edge_types: &[String],
) -> Result<Vec<SearchResult>, TraversalError> {
    info!("Starting Phase 2: Graph expansion from {} vector hits", vector_hits.len());

    let mut all_results = Vec::new();
    let mut expansion_tasks = Vec::new();

    for hit in vector_hits {
        let client = Arc::clone(&client);
        let query_embedding = query_embedding.to_vec();
        let hit = hit.clone();
        let edge_types = edge_types.to_vec();

        let task = tokio::spawn(async move {
            let mut visited = HashSet::new();
            visited.insert(hit.memory_id.clone());
            
            expand_from_node(
                client,
                &hit.memory_id,
                &query_embedding,
                1,
                max_depth,
                &mut visited,
                hit.combined_score,
            ).await
        });

        expansion_tasks.push(task);
    }

    
    for task in expansion_tasks {
        match task.await {
            Ok(Ok(results)) => all_results.extend(results),
            Ok(Err(e)) => warn!("Graph expansion failed: {}", e),
            Err(e) => warn!("Graph expansion task panicked: {}", e),
        }
    }

    info!("Phase 2 completed: {} expanded results", all_results.len());
    Ok(all_results)
}


async fn expand_from_node(
    client: Arc<HelixClient>,
    node_id: &str,
    query_embedding: &[f32],
    current_depth: u32,
    max_depth: u32,
    visited: &mut HashSet<String>,
    parent_score: f64,
) -> Result<Vec<SearchResult>, TraversalError> {
    debug!("Expanding from node {} at depth {}", node_id, current_depth);

    let params = serde_json::json!({
        "memory_id": node_id
    });

    let response: GraphConnectionsResponse = client
        .execute_query("getMemoryLogicalConnections", &params)
        .await
        .map_err(|e| TraversalError::Database(e.to_string()))?;

    let mut results = Vec::new();
    let mut neighbors = Vec::new();

    
    process_edge_collection(
        &response.implies_out,
        "IMPLIES",
        edge_weights::IMPLIES,
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        &response.because_out,
        "BECAUSE",
        edge_weights::BECAUSE,
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        &response.contradicts_out,
        "CONTRADICTS",
        edge_weights::CONTRADICTS,
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        &response.relation_out,
        "MEMORY_RELATION",
        edge_weights::MEMORY_RELATION,
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    
    process_edge_collection(
        &response.implies_in,
        "IMPLIES_IN",
        edge_weights::IMPLIES * 0.9,
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        &response.because_in,
        "BECAUSE_IN",
        edge_weights::BECAUSE * 0.85,
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        &response.contradicts_in,
        "CONTRADICTS_IN",
        edge_weights::CONTRADICTS * 0.8,
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        &response.relation_in,
        "MEMORY_RELATION_IN",
        edge_weights::MEMORY_RELATION * 0.6,
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    
    if current_depth < max_depth {
        
        neighbors.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        for (neighbor_id, neighbor_score) in neighbors.into_iter().take(3) {
            if !visited.contains(&neighbor_id) {
                visited.insert(neighbor_id.clone());
                let expanded = Box::pin(expand_from_node(
                    Arc::clone(&client),
                    &neighbor_id,
                    query_embedding,
                    current_depth + 1,
                    max_depth,
                    visited,
                    neighbor_score,
                )).await?;
                results.extend(expanded);
            }
        }
    }

    Ok(results)
}


fn process_edge_collection(
    memories: &[ConnectedMemory],
    edge_type: &str,
    edge_weight: f64,
    parent_score: f64,
    visited: &HashSet<String>,
    results: &mut Vec<SearchResult>,
    neighbors: &mut Vec<(String, f64)>,
) {
    for mem in memories {
        if visited.contains(&mem.memory_id) {
            continue;
        }

        let temporal_score = calculate_temporal_freshness(&mem.created_at, 30.0);
        let graph_score = calculate_graph_score(edge_weight, parent_score);
        
        
        let semantic_sim = 0.5;
        
        let result = SearchResult::from_graph(
            &mem.memory_id,
            &mem.content,
            semantic_sim,
            graph_score,
            temporal_score,
            1, 
            vec![edge_type.to_string()],
        );

        results.push(result);
        neighbors.push((mem.memory_id.clone(), graph_score));
    }
}


pub fn rank_and_filter(
    results: Vec<SearchResult>,
    min_combined_score: f64,
) -> Vec<SearchResult> {
    info!("Starting Phase 3: Ranking and filtering {} results", results.len());

    
    let mut best_scores: std::collections::HashMap<String, SearchResult> = std::collections::HashMap::new();
    
    for result in results {
        match best_scores.get(&result.memory_id) {
            Some(existing) => {
                if result.combined_score > existing.combined_score {
                    best_scores.insert(result.memory_id.clone(), result);
                }
            }
            None => {
                best_scores.insert(result.memory_id.clone(), result);
            }
        }
    }

    
    let mut filtered_results: Vec<SearchResult> = best_scores
        .into_values()
        .filter(|r| r.combined_score >= min_combined_score)
        .collect();

    
    filtered_results.sort_by(|a, b| b.combined_score.partial_cmp(&a.combined_score).unwrap());

    info!("Phase 3 completed: {} final results", filtered_results.len());
    filtered_results
}