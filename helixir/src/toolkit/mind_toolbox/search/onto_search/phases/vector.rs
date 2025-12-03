

use std::collections::HashSet;
use tracing::{info, warn};
use crate::db::HelixClient;
use super::super::config::OntoSearchConfig;
use super::super::models::OntoSearchResult;
use super::super::temporal::{is_within_temporal_window, calculate_temporal_freshness};


pub async fn vector_search_phase(
    client: &HelixClient,
    query_embedding: &[f32],
    user_id: Option<&str>,
    config: &OntoSearchConfig,
) -> Vec<OntoSearchResult> {
    let params = serde_json::json!({
        "query_vector": query_embedding,
        "limit": config.vector_top_k,
    });

    #[derive(serde::Deserialize)]
    struct VectorResult {
        #[serde(default)]
        memories: Vec<serde_json::Value>,
        #[serde(default)]
        parent_memories: Vec<serde_json::Value>,
    }

    let result: VectorResult = match client.execute_query("smartVectorSearchWithChunks", &params).await {
        Ok(r) => r,
        Err(e) => {
            warn!("Vector search failed: {}", e);
            return Vec::new();
        }
    };

    let mut results = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();

    let all_memories = result.memories.into_iter().chain(result.parent_memories);

    for mem in all_memories {
        let Some(memory_id) = mem.get("memory_id").and_then(|v| v.as_str()) else { continue };
        if seen_ids.contains(memory_id) { continue; }
        seen_ids.insert(memory_id.to_string());

        
        if let Some(uid) = user_id {
            if let Some(mem_uid) = mem.get("user_id").and_then(|v| v.as_str()) {
                if mem_uid != uid { continue; }
            }
        }

        
        let created_at = mem.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
        if !is_within_temporal_window(created_at, config.temporal_hours) { continue; }

        let temporal_score = calculate_temporal_freshness(created_at, config.temporal_decay_rate);

        results.push(OntoSearchResult {
            memory_id: memory_id.to_string(),
            content: mem.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            memory_type: mem.get("memory_type").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            user_id: mem.get("user_id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            vector_score: mem.get("score").and_then(|v| v.as_f64()).unwrap_or(0.8),
            temporal_score,
            created_at: created_at.to_string(),
            source: "vector".to_string(),
            ..Default::default()
        });
    }

    info!("Vector search: {} results", results.len());
    results
}

