use std::sync::Arc;
use std::collections::HashMap;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::utils::nullable_string;
use tracing::{debug, info, warn};

use super::models::{SearchResult, SearchMethod};
use super::cache::SearchCache;
use crate::db::HelixClient;

#[derive(Error, Debug)]
pub enum VectorSearchError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Cache failed: {0}")]
    CacheFailed(String),
}

#[derive(Serialize, Deserialize)]
struct VectorSearchInput {
    query: String,
    user_id: String,
    limit: usize,
    min_score: f64,
}

#[derive(Serialize, Deserialize)]
struct VectorSearchMemory {
    #[serde(default, deserialize_with = "nullable_string")]
    memory_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    content: String,
    #[serde(default)]
    similarity_score: f64,
    #[serde(default, deserialize_with = "nullable_string")]
    memory_type: String,
    #[serde(default, deserialize_with = "nullable_string")]
    user_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    created_at: String,
    #[serde(default, deserialize_with = "nullable_string")]
    updated_at: String,
    #[serde(default, deserialize_with = "nullable_string")]
    valid_from: String,
    #[serde(default)]
    valid_until: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct VectorSearchOutput {
    memories: Vec<VectorSearchMemory>,
}

pub struct VectorSearch {
    client: Arc<HelixClient>,
    cache: SearchCache<Vec<SearchResult>>,
}

impl VectorSearch {
    pub fn new(client: Arc<HelixClient>, cache_size: usize, cache_ttl: u64) -> Self {
        Self {
            client,
            cache: SearchCache::new(cache_size, cache_ttl),
        }
    }

    fn make_cache_key(&self, query: &str, user_id: Option<&str>, limit: usize, min_score: f64) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let key_data = format!("{}|{}|{}|{}", query, user_id.unwrap_or(""), limit, min_score);
        let mut hasher = DefaultHasher::new();
        key_data.hash(&mut hasher);
        format!("{:x}", hasher.finish())[..16].to_string()
    }

    pub async fn search(
        &self,
        query: &str,
        user_id: Option<&str>,
        limit: usize,
        min_score: f64,
        use_cache: bool,
    ) -> Result<Vec<SearchResult>, VectorSearchError> {
        if use_cache {
            let cache_key = self.make_cache_key(query, user_id, limit, min_score);
            if let Some(cached) = self.cache.get(&cache_key) {
                debug!("Vector search cache HIT for: {}", crate::safe_truncate(query, 50));
                return Ok(cached);
            }
        }

        let input = VectorSearchInput {
            query: query.to_string(),
            user_id: user_id.unwrap_or("").to_string(),
            limit,
            min_score,
        };

        let result: VectorSearchOutput = self.client
            .execute_query("vectorSearch", &input)
            .await
            .map_err(|e| VectorSearchError::Database(e.to_string()))?;

        let mut results = Vec::new();
        for item in result.memories {
            let mut metadata = HashMap::new();
            metadata.insert("embedding_distance".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(0.0).unwrap()));
            
            let search_result = SearchResult {
                memory_id: item.memory_id.clone(),
                content: item.content.clone(),
                score: item.similarity_score,
                method: SearchMethod::Vector,
                metadata,
                created_at: item.created_at.clone(),
            };
            results.push(search_result);
        }

        if use_cache {
            let cache_key = self.make_cache_key(query, user_id, limit, min_score);
            self.cache.set(&cache_key, results.clone());
        }

        info!("Vector search returned {} results", results.len());
        Ok(results)
    }
}