use serde::Serialize;
use tracing::{debug, warn};

use super::ToolingManager;

pub(crate) fn safe_truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

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
            .and_then(|m| if m.memory_type.is_empty() { None } else { Some(m.memory_type) })
    }

    pub(crate) async fn ensure_user_exists(&self, user_id: &str) {
        #[derive(serde::Deserialize)]
        struct UserResponse {
            #[serde(default)]
            user: Option<serde_json::Value>,
        }

        let exists = self.db
            .execute_query::<UserResponse, _>(
                "getUser",
                &serde_json::json!({"user_id": user_id}),
            )
            .await
            .map(|r| r.user.is_some())
            .unwrap_or(false);

        if !exists {
            let _ = self.db
                .execute_query::<serde_json::Value, _>(
                    "addUser",
                    &serde_json::json!({"user_id": user_id, "name": user_id}),
                )
                .await;
            debug!("Created user node: {}", user_id);
        }
    }

    pub(crate) async fn link_memory_to_concept(
        &self,
        memory_id: &str,
        concept_id: &str,
        confidence: i32,
    ) -> Result<(), super::ToolingError> {
        #[derive(serde::Deserialize)]
        struct LinkResponse {
            #[serde(default)]
            link: serde_json::Value,
        }

        self.db
            .execute_query::<LinkResponse, _>(
                "linkMemoryToInstanceOf",
                &serde_json::json!({
                    "memory_id": memory_id,
                    "concept_id": concept_id,
                    "confidence": confidence as i64,
                }),
            )
            .await
            .map_err(|e| super::ToolingError::Database(e.to_string()))?;

        debug!("Linked memory {} to concept {}", memory_id, concept_id);
        Ok(())
    }

    pub(crate) async fn update_memory_internal(
        &self,
        memory_id: &str,
        new_content: &str,
        vector: &[f32],
    ) -> Result<(), super::ToolingError> {
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
            .execute_query::<serde_json::Value, _>("updateMemory", &UpdateInput {
                memory_id: memory_id.to_string(),
                content: new_content.to_string(),
                certainty: self.config.default_certainty as i64,
                importance: self.config.default_importance as i64,
                updated_at: now,
            })
            .await
            .map_err(|e| super::ToolingError::Database(e.to_string()))?;

        if let Err(e) = self.db
            .execute_query::<serde_json::Value, _>("deleteMemoryEmbedding", &serde_json::json!({
                "memory_id": memory_id
            }))
            .await
        {
            debug!("No old embedding to delete for {}: {}", memory_id, e);
        }

        let internal_id = {
            #[derive(serde::Deserialize)]
            struct MemResp { memory: MemNode }
            #[derive(serde::Deserialize)]
            struct MemNode { id: String }
            match self.db.execute_query::<MemResp, _>("getMemory", &serde_json::json!({"memory_id": memory_id})).await {
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
        if let Err(e) = self.db
            .execute_query::<serde_json::Value, _>("addMemoryEmbedding", &EmbedInput {
                memory_id: internal_id,
                vector_data: vector.iter().map(|&x| x as f64).collect(),
                embedding_model: self.embedder.model().to_string(),
                created_at: now2,
            })
            .await
        {
            warn!("Failed to update embedding for {}: {}", memory_id, e);
        }

        debug!("Updated memory: {}", memory_id);
        Ok(())
    }

    pub(crate) async fn link_user_to_existing_memory(&self, user_id: &str, memory_id: &str) {
        self.ensure_user_exists(user_id).await;

        #[derive(Serialize)]
        struct LinkInput {
            user_id: String,
            memory_id: String,
            context: String,
        }

        if let Err(e) = self.db
            .execute_query::<serde_json::Value, _>("linkUserToMemory", &LinkInput {
                user_id: user_id.to_string(),
                memory_id: memory_id.to_string(),
                context: "cross_user_link".to_string(),
            })
            .await
        {
            warn!("Failed to cross-link user {} to memory {}: {}", user_id, memory_id, e);
            return;
        }

        #[derive(serde::Deserialize)]
        struct UsersResult {
            #[serde(default)]
            users: Vec<serde_json::Value>,
        }
        let user_count = match self.db
            .execute_query::<UsersResult, _>("getMemoryUsers", &serde_json::json!({"memory_id": memory_id}))
            .await
        {
            Ok(r) => r.users.len().max(1) as i64,
            Err(_) => 2,
        };

        #[derive(Serialize)]
        struct UpdateCountInput {
            memory_id: String,
            user_count: i64,
            updated_at: String,
        }
        let _ = self.db
            .execute_query::<serde_json::Value, _>("updateMemoryUserCount", &UpdateCountInput {
                memory_id: memory_id.to_string(),
                user_count,
                updated_at: chrono::Utc::now().to_rfc3339(),
            })
            .await;

        debug!("Cross-linked user {} to memory {} (user_count={})", user_id, memory_id, user_count);
    }

    pub(crate) async fn link_memory_to_session(
        &self,
        memory_id: &str,
        session_id: &str,
        sequence: i64,
    ) -> Result<(), super::ToolingError> {
        let params = serde_json::json!({
            "memory_id": memory_id,
            "session_id": session_id,
            "sequence": sequence
        });
        self.db
            .execute_query::<serde_json::Value, _>("linkMemoryToSession", &params)
            .await
            .map_err(|e| super::ToolingError::Database(e.to_string()))?;
        debug!("Linked memory {} to session {}", memory_id, session_id);
        Ok(())
    }

    pub(crate) async fn link_agent_to_memory(
        &self,
        agent_id: &str,
        memory_id: &str,
        method: &str,
    ) -> Result<(), super::ToolingError> {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let params = serde_json::json!({
            "agent_id": agent_id,
            "memory_id": memory_id,
            "timestamp": timestamp,
            "method": method
        });
        self.db
            .execute_query::<serde_json::Value, _>("linkAgentToMemory", &params)
            .await
            .map_err(|e| super::ToolingError::Database(e.to_string()))?;
        debug!("Linked agent {} to memory {} (method={})", agent_id, memory_id, method);
        Ok(())
    }

    pub(crate) async fn add_memory_history_event(
        &self,
        memory_id: &str,
        action: &str,
        old_value: &str,
        new_value: &str,
        actor: &str,
    ) -> Result<(), super::ToolingError> {
        let event_id = uuid::Uuid::new_v4().to_string();
        let timestamp = chrono::Utc::now().to_rfc3339();
        let params = serde_json::json!({
            "memory_id": memory_id,
            "event_id": event_id,
            "action": action,
            "old_value": old_value,
            "new_value": new_value,
            "timestamp": timestamp,
            "actor": actor
        });
        self.db
            .execute_query::<serde_json::Value, _>("addMemoryHistoryEvent", &params)
            .await
            .map_err(|e| super::ToolingError::Database(e.to_string()))?;
        debug!("History event for memory {}: {} by {}", memory_id, action, actor);
        Ok(())
    }

    pub(crate) async fn add_entity_relation(
        &self,
        from_entity_id: &str,
        to_entity_id: &str,
        relationship_type: &str,
        strength: i64,
    ) -> Result<(), super::ToolingError> {
        let params = serde_json::json!({
            "from_id": from_entity_id,
            "to_id": to_entity_id,
            "relationship_type": relationship_type,
            "strength": strength,
            "bidirectional": 0
        });
        self.db
            .execute_query::<serde_json::Value, _>("addEntityRelation", &params)
            .await
            .map_err(|e| super::ToolingError::Database(e.to_string()))?;
        debug!(
            "Entity relation: {} -[{}]-> {}",
            from_entity_id, relationship_type, to_entity_id
        );
        Ok(())
    }

    pub(crate) async fn add_entity_part_of(
        &self,
        part_id: &str,
        whole_id: &str,
    ) -> Result<(), super::ToolingError> {
        let params = serde_json::json!({
            "part_id": part_id,
            "whole_id": whole_id
        });
        self.db
            .execute_query::<serde_json::Value, _>("addEntityPartOf", &params)
            .await
            .map_err(|e| super::ToolingError::Database(e.to_string()))?;
        debug!("Entity part-of: {} is part of {}", part_id, whole_id);
        Ok(())
    }

    pub(crate) async fn add_memory_valid_in_context(
        &self,
        memory_id: &str,
        context_id: &str,
        priority: i64,
    ) -> Result<(), super::ToolingError> {
        let params = serde_json::json!({
            "memory_id": memory_id,
            "context_id": context_id,
            "priority": priority,
            "exclusive": 0
        });
        self.db
            .execute_query::<serde_json::Value, _>("addMemoryValidIn", &params)
            .await
            .map_err(|e| super::ToolingError::Database(e.to_string()))?;
        debug!("Memory {} valid in context {} (priority={})", memory_id, context_id, priority);
        Ok(())
    }

    pub(crate) async fn add_concept_is_a(
        &self,
        child_id: &str,
        parent_id: &str,
        inheritance_type: &str,
    ) -> Result<(), super::ToolingError> {
        let params = serde_json::json!({
            "child_id": child_id,
            "parent_id": parent_id,
            "inheritance_type": inheritance_type
        });
        self.db
            .execute_query::<serde_json::Value, _>("addConceptIsA", &params)
            .await
            .map_err(|e| super::ToolingError::Database(e.to_string()))?;
        debug!("Concept IS_A: {} -> {}", child_id, parent_id);
        Ok(())
    }

    pub(crate) async fn add_concept_relation(
        &self,
        from_id: &str,
        to_id: &str,
        relation_type: &str,
    ) -> Result<(), super::ToolingError> {
        let params = serde_json::json!({
            "from_id": from_id,
            "to_id": to_id,
            "relation_type": relation_type
        });
        self.db
            .execute_query::<serde_json::Value, _>("addConceptRelation", &params)
            .await
            .map_err(|e| super::ToolingError::Database(e.to_string()))?;
        debug!("Concept relation: {} -[{}]-> {}", from_id, relation_type, to_id);
        Ok(())
    }

    pub(crate) async fn add_cross_user_contradiction(
        &self,
        new_memory_id: &str,
        existing_memory_id: &str,
        conflict_type: &str,
        reasoning: &str,
    ) {
        #[derive(Serialize)]
        struct ContradictInput {
            from_id: String,
            to_id: String,
            resolution: String,
            resolved: i64,
            resolution_strategy: String,
        }

        if let Err(e) = self.db
            .execute_query::<serde_json::Value, _>("addMemoryContradiction", &ContradictInput {
                from_id: new_memory_id.to_string(),
                to_id: existing_memory_id.to_string(),
                resolution: reasoning.to_string(),
                resolved: 0,
                resolution_strategy: format!("cross_user_{}", conflict_type),
            })
            .await
        {
            warn!(
                "Failed to add cross-user contradiction {} -> {}: {}",
                new_memory_id, existing_memory_id, e
            );
        } else {
            debug!(
                "Added cross-user contradiction: {} -> {} (type={})",
                new_memory_id, existing_memory_id, conflict_type
            );
        }
    }
}
