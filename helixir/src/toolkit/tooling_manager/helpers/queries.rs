//! Live read/write helpers against `Memory` nodes.

use serde::Serialize;
use tracing::{debug, warn};

use super::super::{ToolingError, ToolingManager};

impl ToolingManager {
    /// Load the persisted fields that make C2/C4 constitutional guards.
    /// Destructive callers propagate database errors and therefore fail closed.
    pub(crate) async fn get_memory_protection(
        &self,
        memory_id: &str,
    ) -> Result<crate::core::charter::TargetProtection, ToolingError> {
        #[derive(serde::Deserialize)]
        struct GetMemoryResponse {
            #[serde(default)]
            memory: Option<MemoryFields>,
        }
        #[derive(serde::Deserialize)]
        struct MemoryFields {
            #[serde(default)]
            immutable: Option<i64>,
            #[serde(default, deserialize_with = "crate::utils::nullable_string")]
            source: String,
        }

        let response = self
            .db
            .execute_query::<GetMemoryResponse, _>(
                "getMemory",
                &serde_json::json!({"memory_id": memory_id}),
            )
            .await
            .map_err(|error| ToolingError::Database(error.to_string()))?;
        let memory = response.memory.ok_or_else(|| {
            ToolingError::Memory(format!("charter target memory {memory_id} not found"))
        })?;
        Ok(crate::core::charter::target_protection(
            memory.immutable.unwrap_or(0),
            &memory.source,
        ))
    }

    /// Promote a memory into the C2 immutable set.
    pub(crate) async fn set_memory_immutable(&self, memory_id: &str) -> Result<(), ToolingError> {
        self.db
            .execute_query::<serde_json::Value, _>(
                "setMemoryImmutable",
                &serde_json::json!({"memory_id": memory_id, "immutable": 1}),
            )
            .await
            .map_err(|error| ToolingError::Database(error.to_string()))?;
        Ok(())
    }

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

        #[derive(serde::Deserialize)]
        struct UpdateResult {
            #[serde(default)]
            updated: Option<serde_json::Value>,
        }
        let result = self
            .db
            .execute_query::<UpdateResult, _>(
                "updateMutableMemory",
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
        if result.updated.is_none() {
            return Err(ToolingError::Memory(format!(
                "charter rejected concurrent update of {memory_id}"
            )));
        }

        // Resolve the node's internal UUID FIRST: `deleteMemoryEmbedding` is
        // declared `memory_id: ID` (internal UUID), so passing the mem_… string
        // always failed with a Decode error that was swallowed below — leaving
        // the OLD embedding alive next to the new one on every update. A stale
        // embedding keeps matching vector searches with content the memory no
        // longer holds (violates the HAS_EMBEDDING-is-1:1 invariant).
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

        if let Err(e) = self
            .db
            .execute_query::<serde_json::Value, _>(
                "deleteMemoryEmbedding",
                &serde_json::json!({
                    "memory_id": internal_id
                }),
            )
            .await
        {
            // A genuinely-new memory has no embedding yet — that's the only
            // expected miss now that the id type is correct.
            debug!("No old embedding to delete for {}: {}", memory_id, e);
        }

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
