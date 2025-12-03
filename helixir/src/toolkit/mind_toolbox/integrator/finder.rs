use std::sync::Arc;
use chrono::{DateTime, Utc};
use crate::db::HelixClient;
use serde::{Serialize, Deserialize};
use thiserror::Error;
use tracing::{debug, info, warn};

use super::models::SimilarMemory;
use super::similarity::cosine_similarity;

#[derive(Error, Debug)]
pub enum FinderError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("No results found")]
    NoResults,
}

#[derive(Debug, Serialize, Deserialize)]
struct SearchResult {
    memory_id: String,
    content: String,
    user_id: Option<String>,
    created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct VectorSearchResponse {
    memories: Vec<SearchResult>,
    parent_memories: Vec<SearchResult>,
}

pub struct SimilarMemoryFinder {
    client: Arc<HelixClient>,
    similarity_threshold: f64,
    max_similar: usize,
}

impl SimilarMemoryFinder {
    pub fn new(client: Arc<HelixClient>, threshold: f64, max_similar: usize) -> Self {
        Self {
            client,
            similarity_threshold: threshold,
            max_similar,
        }
    }

    pub async fn find_similar(
        &self,
        query_embedding: &[f32],
        user_id: &str,
        exclude_id: Option<&str>,
    ) -> Result<Vec<SimilarMemory>, FinderError> {
        let response: VectorSearchResponse = self
            .client
            .execute_query("smartVectorSearchWithChunks", &serde_json::json!({
                "query_vector": query_embedding,
                "limit": self.max_similar * 2
            }))
            .await
            .map_err(|e| FinderError::Database(e.to_string()))?;

        let mut all_memories = response.memories;
        all_memories.extend(response.parent_memories);

        let mut seen_ids = std::collections::HashSet::new();
        let mut candidates = Vec::new();

        for memory in all_memories {
            if seen_ids.contains(&memory.memory_id) {
                continue;
            }
            seen_ids.insert(memory.memory_id.clone());

            if Some(memory.memory_id.as_str()) == exclude_id {
                continue;
            }

            let mem_user_id = memory.user_id.as_deref().unwrap_or("unknown");
            if mem_user_id != user_id {
                continue;
            }

            let score = 0.8f64;
            if score >= self.similarity_threshold {
                let created_at = memory.created_at
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now());

                candidates.push(SimilarMemory {
                    memory_id: memory.memory_id,
                    content: memory.content,
                    embedding: query_embedding.to_vec(),
                    similarity_score: score,
                    created_at,
                });
            }
        }

        candidates.sort_by(|a, b| b.similarity_score.partial_cmp(&a.similarity_score).unwrap());

        candidates.truncate(self.max_similar);

        if candidates.is_empty() {
            warn!("No similar memories found for user {}", user_id);
            return Err(FinderError::NoResults);
        }

        info!("Found {} similar memories", candidates.len());
        Ok(candidates)
    }
}