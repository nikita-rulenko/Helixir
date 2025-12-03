use chrono::Utc;
use helix_rs::HelixDB;
use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, warn};

use super::contradiction::ContradictionDetector;
use super::crud::MemoryCrud;
use super::relations::RelationCopier;

#[derive(Error, Debug)]
pub enum SupersessionError {
    #[error("Memory not found: {0}")]
    MemoryNotFound(String),
    #[error("Failed to create new memory: {0}")]
    CreationFailed(String),
    #[error("Failed to create supersession edge: {0}")]
    EdgeCreationFailed(String),
    #[error("Database error: {0}")]
    DatabaseError(#[from] helix_rs::HelixError),
}

#[derive(Debug)]
pub struct SupersessionResult {
    pub new_memory_id: String,
    pub old_memory_id: String,
    pub is_contradiction: bool,
    pub relations_copied: usize,
}

pub struct SupersessionManager {
    client: HelixDB,
    memory_crud: MemoryCrud,
    contradiction_detector: ContradictionDetector,
    relation_copier: RelationCopier,
}

impl SupersessionManager {
    pub fn new(client: HelixDB) -> Self {
        Self {
            memory_crud: MemoryCrud::new(client.clone()),
            contradiction_detector: ContradictionDetector::new(),
            relation_copier: RelationCopier::new(client.clone()),
            client,
        }
    }

    pub async fn supersede_memory(
        &self,
        old_memory_id: &str,
        new_content: &str,
        user_id: &str,
        memory_type: &str,
    ) -> Result<SupersessionResult, SupersessionError> {
        let old_memory = self
            .memory_crud
            .get_memory(old_memory_id)
            .await?
            .ok_or_else(|| SupersessionError::MemoryNotFound(old_memory_id.to_string()))?;

        let new_memory_id = format!("mem_{}", uuid::Uuid::new_v4().simple().to_string()[..12].to_string());
        let created_at = Utc::now();

        let new_memory = self
            .memory_crud
            .create_memory(
                &new_memory_id,
                new_content,
                user_id,
                memory_type,
                old_memory.certainty,
                old_memory.importance,
                "supersession",
                &format!(r#"{{"supersedes": "{}"}}"#, old_memory_id),
            )
            .await
            .map_err(|e| SupersessionError::CreationFailed(e.to_string()))?;

        let is_contradiction = self
            .contradiction_detector
            .detect_contradiction(&old_memory.content, new_content);

        let mut params = HashMap::new();
        params.insert("new_id".to_string(), new_memory_id.clone());
        params.insert("old_id".to_string(), old_memory_id.to_string());
        params.insert("reason".to_string(), "content_update".to_string());
        params.insert("superseded_at".to_string(), created_at.to_rfc3339());
        params.insert("is_contradiction".to_string(), if is_contradiction { 1 } else { 0 });

        self.client
            .execute_query("addMemorySupersession", params)
            .await
            .map_err(|e| SupersessionError::EdgeCreationFailed(e.to_string()))?;

        if is_contradiction {
            let mut params = HashMap::new();
            params.insert("from_id".to_string(), new_memory_id.clone());
            params.insert("to_id".to_string(), old_memory_id.to_string());
            params.insert("resolution".to_string(), "superseded".to_string());
            params.insert("resolved".to_string(), 1);
            params.insert("resolution_strategy".to_string(), "newer_wins".to_string());

            if let Err(e) = self.client.execute_query("addMemoryContradiction", params).await {
                warn!("Failed to create contradiction edge: {}", e);
            }
        }

        let relations_copied = self
            .relation_copier
            .copy_relations(old_memory_id, &new_memory_id)
            .await
            .unwrap_or(0);

        debug!(
            "Superseded memory {} -> {} (contradiction: {}, relations: {})",
            old_memory_id, new_memory_id, is_contradiction, relations_copied
        );

        Ok(SupersessionResult {
            new_memory_id,
            old_memory_id: old_memory_id.to_string(),
            is_contradiction,
            relations_copied,
        })
    }

    pub async fn update_metadata_only(
        &self,
        memory_id: &str,
        certainty: Option<i32>,
        importance: Option<i32>,
    ) -> Result<(), SupersessionError> {
        let memory = self
            .memory_crud
            .get_memory(memory_id)
            .await?
            .ok_or_else(|| SupersessionError::MemoryNotFound(memory_id.to_string()))?;

        let update_certainty = certainty.unwrap_or(memory.certainty);
        let update_importance = importance.unwrap_or(memory.importance);

        let mut params = HashMap::new();
        params.insert("id".to_string(), memory.internal_id);
        params.insert("content".to_string(), memory.content);
        params.insert("certainty".to_string(), update_certainty);
        params.insert("importance".to_string(), update_importance);
        params.insert("updated_at".to_string(), Utc::now().to_rfc3339());

        self.client
            .execute_query("updateMemoryById", params)
            .await?;

        debug!("Updated metadata for memory {}", memory_id);
        Ok(())
    }
}