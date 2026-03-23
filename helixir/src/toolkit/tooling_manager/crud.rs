use serde::{Deserialize, Deserializer, Serialize};
use tracing::{info, debug};

use crate::utils::nullable_string;
use super::types::ToolingError;
use super::ToolingManager;

impl ToolingManager {
    pub async fn update_memory(
        &self,
        memory_id: &str,
        new_content: &str,
        _user_id: &str,
    ) -> Result<bool, ToolingError> {
        info!("Updating memory: {}", memory_id);

        let vector = self
            .embedder
            .generate(new_content, true)
            .await
            .map_err(|e| ToolingError::Embedding(e.to_string()))?;

        let now = chrono::Utc::now().to_rfc3339();

        #[derive(serde::Deserialize)]
        struct GetMemResult {
            #[serde(default)]
            memory: Option<MemNode>,
        }
        #[derive(serde::Deserialize)]
        struct MemNode {
            #[serde(default, deserialize_with = "nullable_string")]
            id: String,
        }

        let mem_result: GetMemResult = self.db
            .execute_query("getMemory", &serde_json::json!({"memory_id": memory_id}))
            .await
            .map_err(|e| ToolingError::Database(format!("Failed to get memory: {}", e)))?;

        let internal_id = match mem_result.memory {
            Some(m) if !m.id.is_empty() => m.id,
            _ => return Err(ToolingError::Database(format!("Memory {} not found", memory_id))),
        };

        #[derive(Serialize)]
        struct UpdateByIdParams {
            id: String,
            content: String,
            certainty: i64,
            importance: i64,
            updated_at: String,
        }

        let params = UpdateByIdParams {
            id: internal_id.clone(),
            content: new_content.to_string(),
            certainty: 80,
            importance: 50,
            updated_at: now.clone(),
        };

        let _result: serde_json::Value = self.db
            .execute_query("updateMemoryById", &params)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        debug!("Memory {} (id={}) updated successfully", memory_id, internal_id);

        #[derive(serde::Deserialize)]
        struct MemoryResult {
            #[serde(default)]
            memory: Option<MemoryData>,
        }
        #[derive(serde::Deserialize)]
        struct MemoryData {
            #[serde(default, deserialize_with = "nullable_string")]
            id: String,
        }

        if let Ok(result) = self.db.execute_query::<MemoryResult, _>(
            "getMemory",
            &serde_json::json!({"memory_id": memory_id}),
        ).await {
            if let Some(mem) = result.memory {
                if !mem.id.is_empty() {
                    #[derive(serde::Deserialize)]
                    struct EmbeddingResult {
                        #[serde(default)]
                        embedding: serde_json::Value,
                    }

                    let _ = self.db.execute_query::<EmbeddingResult, _>(
                        "addMemoryEmbedding",
                        &serde_json::json!({
                            "memory_id": mem.id,
                            "vector_data": vector.iter().map(|&x| x as f64).collect::<Vec<f64>>(),
                            "embedding_model": self.embedder.model(),
                            "created_at": now,
                        }),
                    ).await;
                }
            }
        }

        Ok(true)
    }

    pub async fn delete_memory(&self, memory_id: &str) -> Result<bool, ToolingError> {
        info!("Deleting memory: {}", memory_id);

        #[derive(Serialize)]
        struct DeleteInput {
            memory_id: String,
        }

        self.db
            .execute_query::<(), _>("deleteMemory", &DeleteInput {
                memory_id: memory_id.to_string(),
            })
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        Ok(true)
    }
}
