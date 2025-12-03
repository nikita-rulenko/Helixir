use chrono::{DateTime, Utc};
use crate::db::HelixClient;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::llm::embeddings::EmbeddingGenerator;
use super::models::Memory;

#[derive(Error, Debug)]
pub enum CrudError {
    #[error("HelixDB error: {0}")]
    HelixDB(String),
    #[error("Embedding generation error: {0}")]
    Embedding(String),
    #[error("Missing internal ID from addMemory result")]
    MissingInternalId,
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl From<crate::db::HelixClientError> for CrudError {
    fn from(e: crate::db::HelixClientError) -> Self {
        CrudError::HelixDB(e.to_string())
    }
}

#[derive(Serialize)]
struct AddMemoryInput {
    memory_id: String,
    user_id: String,
    content: String,
    memory_type: String,
    created_at: String,
    updated_at: String,
    certainty: i64,
    importance: i64,
    context_tags: String,
    source: String,
    metadata: String,
}

#[derive(Deserialize)]
struct AddMemoryOutput {
    memory: MemoryNode,
}

#[derive(Deserialize)]
struct MemoryNode {
    id: String,
    memory_id: String,
}

#[derive(Serialize)]
struct AddEmbeddingInput {
    memory_id: String,
    vector_data: Vec<f32>,
    embedding_model: String,
    created_at: String,
}

#[derive(Serialize)]
struct GetMemoryInput {
    memory_id: String,
}

#[derive(Deserialize)]
struct GetMemoryOutput {
    memory: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct LinkUserMemoryInput {
    user_id: String,
    memory_id: String,
    context: String,
}

#[derive(Serialize)]
struct AddUserInput {
    user_id: String,
    name: String,
}

pub struct MemoryCrud {
    client: HelixClient,
    embedder: Option<Arc<EmbeddingGenerator>>,
}

impl MemoryCrud {
    pub fn new(client: HelixClient, embedder: Option<Arc<EmbeddingGenerator>>) -> Self {
        info!("MemoryCrud initialized (embedder={})", embedder.is_some());
        Self { client, embedder }
    }

    pub async fn add_memory(
        &self,
        content: String,
        user_id: String,
        memory_type: Option<String>,
        certainty: Option<i64>,
        importance: Option<i64>,
        source: Option<String>,
        context_tags: Option<String>,
        metadata: Option<String>,
    ) -> Result<Memory, CrudError> {
        let memory_id = format!("mem_{}", Uuid::new_v4().to_string().chars().take(12).collect::<String>());
        let now = Utc::now().to_rfc3339();
        
        let input = AddMemoryInput {
            memory_id: memory_id.clone(),
            user_id: user_id.clone(),
            content: content.clone(),
            memory_type: memory_type.unwrap_or_else(|| "fact".to_string()),
            created_at: now.clone(),
            updated_at: now.clone(),
            certainty: certainty.unwrap_or(80),
            importance: importance.unwrap_or(50),
            context_tags: context_tags.unwrap_or_default(),
            source: source.unwrap_or_else(|| "user".to_string()),
            metadata: metadata.unwrap_or_else(|| "{}".to_string()),
        };

        let result: AddMemoryOutput = self.client.execute_query("addMemory", &input).await?;
        let internal_id = result.memory.id;
        
        if internal_id.is_empty() {
            return Err(CrudError::MissingInternalId);
        }

        debug!("Memory created: {} (internal: {})", memory_id, internal_id);

        if let Some(ref embedder) = self.embedder {
            match embedder.generate(&content, true).await {
                Ok(vector) => {
                    let embed_input = AddEmbeddingInput {
                        memory_id: internal_id.clone(),
                        vector_data: vector,
                        embedding_model: embedder.model(),
                        created_at: now.clone(),
                    };
                    if let Err(e) = self.client.execute_query::<(), _>("addMemoryEmbedding", &embed_input).await {
                        warn!("Failed to create embedding for {}: {}", memory_id, e);
                    } else {
                        debug!("Embedding created for {}", memory_id);
                    }
                }
                Err(e) => warn!("Failed to generate embedding for {}: {}", memory_id, e),
            }
        }

        if let Err(_) = self.client.execute_query::<serde_json::Value, _>("getUser", &serde_json::json!({"user_id": user_id.clone()})).await {
            let user_input = AddUserInput { user_id: user_id.clone(), name: user_id.clone() };
            if let Err(e) = self.client.execute_query::<(), _>("addUser", &user_input).await {
                warn!("Failed to create user {}: {}", user_id, e);
            } else {
                debug!("Created user {}", user_id);
            }
        }

        let link_input = LinkUserMemoryInput {
            user_id,
            memory_id: memory_id.clone(),
            context: "created".to_string(),
        };
        if let Err(e) = self.client.execute_query::<(), _>("linkUserToMemory", &link_input).await {
            warn!("Failed to link memory to user: {}", e);
        } else {
            debug!("Linked memory {} to user", memory_id);
        }

        let memory = Memory {
            memory_id,
            content,
            memory_type: input.memory_type,
            user_id: input.user_id,
            certainty: input.certainty,
            importance: input.importance,
            created_at: now.clone(),
            updated_at: now.clone(),
            valid_from: now,
            valid_until: String::new(),
            immutable: 0,
            verified: 0,
            context_tags: input.context_tags,
            source: input.source,
            metadata: input.metadata,
            is_deleted: 0,
            deleted_at: String::new(),
            deleted_by: String::new(),
            concepts: Vec::new(),
        };
        
        Ok(memory)
    }

    pub async fn get_memory(&self, memory_id: &str) -> Result<Option<Memory>, CrudError> {
        let input = GetMemoryInput { memory_id: memory_id.to_string() };
        let result: GetMemoryOutput = self.client.execute_query("getMemory", &input).await?;
        
        if let Some(data) = result.memory {
            let memory: Memory = serde_json::from_value(data)?;
            Ok(Some(memory))
        } else {
            Ok(None)
        }
    }

    pub async fn get_memory_by_internal_id(&self, internal_id: &str) -> Result<Option<Memory>, CrudError> {
        warn!("get_memory_by_internal_id not implemented - requires HelixDB query by internal ID");
        Ok(None)
    }

    pub async fn delete_memory(&self, memory_id: &str) -> Result<bool, CrudError> {
        warn!("delete_memory({}) - NOT IMPLEMENTED", memory_id);
        Ok(false)
    }
}