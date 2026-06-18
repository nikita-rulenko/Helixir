//! Persistence layer of the add pipeline.
//!
//! - [`ToolingManager::store_new_memory`] is the canonical write: creates the
//!   `Memory` node, attaches the embedding, links the user, optionally chunks
//!   long content, and ties the memory to its context tag.
//! - [`ToolingManager::store_raw_source`] preserves the original long input
//!   alongside the atomized facts (source="raw_input").

use serde::Serialize;
use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};

use crate::llm::extractor::ExtractedMemory;

use super::super::{ToolingError, ToolingManager};

/// Deterministic content fingerprint for cross-user grouping (#43). Identical
/// normalized (content, memory_type) → identical key, so concurrent writers of
/// the same fact land in one fingerprint group with no coordination — each keeps
/// their own personal node, the collective counts the group as one consensus.
/// Normalization is byte-level (lowercase + whitespace-collapse): it groups exact
/// restatements; semantic paraphrase stays the search/Atropos layer's job.
pub(crate) fn content_key(text: &str, memory_type: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(memory_type.to_lowercase().as_bytes());
    hasher.update([0u8]);
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

impl ToolingManager {
    pub(crate) async fn store_new_memory(
        &self,
        memory: &ExtractedMemory,
        user_id: &str,
        vector: &[f32],
        context_tags: &str,
    ) -> Result<(String, usize), ToolingError> {
        // Memory.user_id must always match the owning user: personal search (e.g. SmartTraversalV2)
        // filters on this field; empty values break isolation until backfilled.
        if user_id.trim().is_empty() {
            return Err(ToolingError::Memory(
                "user_id must be non-empty when creating a Memory node".to_string(),
            ));
        }

        let memory_id = format!(
            "mem_{}",
            uuid::Uuid::new_v4()
                .to_string()
                .replace("-", "")
                .chars()
                .take(12)
                .collect::<String>()
        );
        let now = chrono::Utc::now().to_rfc3339();

        #[derive(Serialize)]
        struct AddMemoryInput {
            memory_id: String,
            content_key: String,
            user_id: String,
            content: String,
            memory_type: String,
            certainty: i64,
            importance: i64,
            created_at: String,
            updated_at: String,
            valid_from: String,
            context_tags: String,
            source: String,
            metadata: String,
        }

        let input = AddMemoryInput {
            memory_id: memory_id.clone(),
            content_key: content_key(&memory.text, &memory.memory_type),
            // Same string as linkUserToMemory — required for vector-hit user filtering.
            user_id: user_id.to_string(),
            content: memory.text.clone(),
            memory_type: memory.memory_type.clone(),
            certainty: memory.certainty as i64,
            importance: memory.importance as i64,
            created_at: now.clone(),
            updated_at: now.clone(),
            valid_from: now.clone(),
            context_tags: context_tags.to_string(),
            source: "llm_extraction".to_string(),
            metadata: "{}".to_string(),
        };

        #[derive(serde::Deserialize)]
        struct AddMemoryResponse {
            memory: MemoryNode,
        }
        #[derive(serde::Deserialize)]
        struct MemoryNode {
            id: String,
        }

        let response: AddMemoryResponse = self
            .db
            .execute_query("addMemoryKeyed", &input)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        let internal_id = response.memory.id;
        debug!("Memory created: {} (internal: {})", memory_id, internal_id);

        #[derive(Serialize)]
        struct AddEmbeddingInput {
            memory_id: String,
            vector_data: Vec<f64>,
            embedding_model: String,
            created_at: String,
        }

        let embed_input = AddEmbeddingInput {
            memory_id: internal_id,
            vector_data: vector.iter().map(|&x| x as f64).collect(),
            embedding_model: self.embedder.model().to_string(),
            created_at: now.clone(),
        };

        if let Err(e) = self
            .db
            .execute_query::<serde_json::Value, _>("addMemoryEmbedding", &embed_input)
            .await
        {
            warn!("Failed to add embedding for {}: {}", memory_id, e);
        } else {
            debug!("Embedding added for {}", memory_id);
        }

        self.ensure_user_exists(user_id).await;

        #[derive(Serialize)]
        struct LinkUserInput {
            user_id: String,
            memory_id: String,
            context: String,
            stance: String,
            certainty: i64,
            linked_at: String,
        }

        if let Err(e) = self
            .db
            .execute_query::<serde_json::Value, _>(
                "linkUserToMemoryWithStance",
                &LinkUserInput {
                    user_id: user_id.to_string(),
                    memory_id: memory_id.clone(),
                    context: "created".to_string(),
                    // Cognitive layer: the creator asserts the fact.
                    stance: "asserts".to_string(),
                    certainty: memory.certainty as i64,
                    linked_at: chrono::Utc::now().to_rfc3339(),
                },
            )
            .await
        {
            warn!(
                "Failed to link user {} to memory {}: {}",
                user_id, memory_id, e
            );
        }

        let mut chunk_count = 0usize;
        if self.chunking_manager.should_chunk(&memory.text) {
            info!(
                "Content exceeds threshold ({} chars), creating chunks",
                memory.text.chars().count()
            );
            match self
                .chunking_manager
                .add_memory_with_chunking(
                    &memory_id,
                    &memory.text,
                    user_id,
                    &memory.memory_type,
                    memory.certainty as i64,
                    memory.importance as i64,
                    "llm_extraction",
                    "",
                    "{}",
                )
                .await
            {
                Ok(result) => {
                    chunk_count = result.chunk_count;
                    info!("Created {} chunks for {}", chunk_count, memory_id);
                }
                Err(e) => {
                    warn!("Failed to chunk memory {}: {}", memory_id, e);
                }
            }
        }

        if let Some(ref context_tag) = memory.context {
            if let Err(e) = self
                .link_memory_to_extracted_context(&memory_id, context_tag)
                .await
            {
                warn!(
                    "Failed to link memory {} to context '{}': {}",
                    memory_id, context_tag, e
                );
            }
        }

        debug!("Stored new memory: {}", memory_id);
        Ok((memory_id, chunk_count))
    }

    /// One-shot migration (#43): stamp a content_key fingerprint onto existing
    /// memories that predate the field, so old facts also group. Idempotent —
    /// already-keyed nodes are skipped, so it is safe to re-run. Returns
    /// (scanned, updated). The hash matches the write path exactly (raw sources
    /// keyed by "raw_input", others by their memory_type).
    pub async fn backfill_content_keys(&self, limit: i64) -> Result<(usize, usize), ToolingError> {
        #[derive(serde::Deserialize)]
        struct Mem {
            #[serde(default)]
            memory_id: String,
            #[serde(default)]
            content: String,
            #[serde(default)]
            memory_type: String,
            #[serde(default)]
            source: String,
            // Legacy rows that predate the field come back as JSON null, which
            // does not deserialize into String — Option absorbs both null and "".
            #[serde(default)]
            content_key: Option<String>,
        }
        #[derive(serde::Deserialize)]
        struct Resp {
            #[serde(default)]
            memories: Vec<Mem>,
        }

        let resp: Resp = self
            .db
            .execute_query("getRecentMemories", &serde_json::json!({ "limit": limit }))
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        let scanned = resp.memories.len();
        let mut updated = 0usize;
        for m in resp.memories {
            let already_keyed = !m.content_key.as_deref().unwrap_or("").is_empty();
            if already_keyed || m.memory_id.is_empty() {
                continue;
            }
            let type_component = if m.source == "raw_input" {
                "raw_input"
            } else {
                m.memory_type.as_str()
            };
            let key = content_key(&m.content, type_component);
            if self
                .db
                .execute_query::<serde_json::Value, _>(
                    "setMemoryContentKey",
                    &serde_json::json!({ "memory_id": m.memory_id, "content_key": key }),
                )
                .await
                .is_ok()
            {
                updated += 1;
            }
        }
        Ok((scanned, updated))
    }

    pub(super) async fn store_raw_source(
        &self,
        memory: &ExtractedMemory,
        user_id: &str,
        vector: &[f32],
        context_tags: &str,
    ) -> Result<String, ToolingError> {
        let memory_id = format!(
            "raw_{}",
            uuid::Uuid::new_v4()
                .to_string()
                .replace("-", "")
                .chars()
                .take(12)
                .collect::<String>()
        );
        let now = chrono::Utc::now().to_rfc3339();

        #[derive(Serialize)]
        struct Input {
            memory_id: String,
            content_key: String,
            user_id: String,
            content: String,
            memory_type: String,
            certainty: i64,
            importance: i64,
            created_at: String,
            updated_at: String,
            valid_from: String,
            context_tags: String,
            source: String,
            metadata: String,
        }

        let input = Input {
            memory_id: memory_id.clone(),
            content_key: content_key(&memory.text, "raw_input"),
            user_id: user_id.to_string(),
            content: memory.text.clone(),
            memory_type: memory.memory_type.clone(),
            certainty: memory.certainty as i64,
            importance: memory.importance as i64,
            created_at: now.clone(),
            updated_at: now.clone(),
            valid_from: now.clone(),
            context_tags: context_tags.to_string(),
            source: "raw_input".to_string(),
            metadata: "{}".to_string(),
        };

        #[derive(serde::Deserialize)]
        struct Resp {
            memory: Node,
        }
        #[derive(serde::Deserialize)]
        struct Node {
            id: String,
        }

        let resp: Resp = self
            .db
            .execute_query("addMemoryKeyed", &input)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        #[derive(Serialize)]
        struct EmbedInput {
            memory_id: String,
            vector_data: Vec<f64>,
            embedding_model: String,
            created_at: String,
        }

        let _ = self
            .db
            .execute_query::<serde_json::Value, _>(
                "addMemoryEmbedding",
                &EmbedInput {
                    memory_id: resp.memory.id,
                    vector_data: vector.iter().map(|&x| x as f64).collect(),
                    embedding_model: self.embedder.model().to_string(),
                    created_at: now,
                },
            )
            .await;

        self.ensure_user_exists(user_id).await;
        let _ = self
            .db
            .execute_query::<serde_json::Value, _>(
                "linkUserToMemoryWithStance",
                &serde_json::json!({
                    "user_id": user_id,
                    "memory_id": memory_id,
                    "context": "raw_source",
                    "stance": "asserts",
                    "certainty": 70,
                    "linked_at": chrono::Utc::now().to_rfc3339(),
                }),
            )
            .await;

        Ok(memory_id)
    }
}
