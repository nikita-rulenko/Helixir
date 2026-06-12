//! Live read/write helpers against `Memory` nodes.

use serde::Serialize;
use tracing::{debug, warn};

use super::super::{ToolingError, ToolingManager};

impl ToolingManager {
    pub(crate) async fn get_memory_type(&self, memory_id: &str) -> Option<String> {
        #[derive(serde::Deserialize)]
        struct GetMemoryResponse {
            #[serde(default)]
            memory: Option<MemoryFields>,
        }

        #[derive(serde::Deserialize)]
        struct MemoryFields {
            #[serde(default)]
            memory_type: String,
        }

        self.db
            .execute_query::<GetMemoryResponse, _>(
                "getMemory",
                &serde_json::json!({"memory_id": memory_id}),
            )
            .await
            .ok()
            .and_then(|r| r.memory)
            .and_then(|m| {
                if m.memory_type.is_empty() {
                    None
                } else {
                    Some(m.memory_type)
                }
            })
    }

    pub(crate) async fn update_memory_internal(
        &self,
        memory_id: &str,
        new_content: &str,
        vector: &[f32],
    ) -> Result<(), ToolingError> {
        #[derive(Serialize)]
        struct UpdateInput {
            memory_id: String,
            content: String,
            certainty: i64,
            importance: i64,
            updated_at: String,
        }

        let now = chrono::Utc::now().to_rfc3339();

        self.db
            .execute_query::<serde_json::Value, _>(
                "updateMemory",
                &UpdateInput {
                    memory_id: memory_id.to_string(),
                    content: new_content.to_string(),
                    certainty: self.config.default_certainty as i64,
                    importance: self.config.default_importance as i64,
                    updated_at: now,
                },
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        if let Err(e) = self
            .db
            .execute_query::<serde_json::Value, _>(
                "deleteMemoryEmbedding",
                &serde_json::json!({
                    "memory_id": memory_id
                }),
            )
            .await
        {
            debug!("No old embedding to delete for {}: {}", memory_id, e);
        }

        let internal_id = {
            #[derive(serde::Deserialize)]
            struct MemResp {
                memory: MemNode,
            }
            #[derive(serde::Deserialize)]
            struct MemNode {
                id: String,
            }
            match self
                .db
                .execute_query::<MemResp, _>(
                    "getMemory",
                    &serde_json::json!({"memory_id": memory_id}),
                )
                .await
            {
                Ok(r) => r.memory.id,
                Err(_) => memory_id.to_string(),
            }
        };

        #[derive(Serialize)]
        struct EmbedInput {
            memory_id: String,
            vector_data: Vec<f64>,
            embedding_model: String,
            created_at: String,
        }
        let now2 = chrono::Utc::now().to_rfc3339();
        if let Err(e) = self
            .db
            .execute_query::<serde_json::Value, _>(
                "addMemoryEmbedding",
                &EmbedInput {
                    memory_id: internal_id,
                    vector_data: vector.iter().map(|&x| x as f64).collect(),
                    embedding_model: self.embedder.model().to_string(),
                    created_at: now2,
                },
            )
            .await
        {
            warn!("Failed to update embedding for {}: {}", memory_id, e);
        }

        debug!("Updated memory: {}", memory_id);
        Ok(())
    }
}
