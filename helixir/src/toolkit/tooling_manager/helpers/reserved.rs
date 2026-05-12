//! Reserved query wrappers: helix queries that exist DB-side but are not
//! yet invoked from the live add/search pipelines.
//!
//! Removing any of these is a public-API regression — the schema is shared
//! with HelixDB. Keep `#[allow(dead_code)]` until the calling sites land.
//!
//! Categories:
//! - `link_memory_to_session` / `link_agent_to_memory` — Session/Agent surfaces.
//! - `add_entity_relation` / `add_entity_part_of` — Entity composition edges.
//! - `add_memory_valid_in_context` — Context-scoped validity (`Constraint`-shaped).
//! - `add_concept_is_a` / `add_concept_relation` — internal concept-graph edges.
//! - `add_cross_user_contradiction` — Hive `CROSS_CONTRADICT` writer.

use serde::Serialize;
use tracing::{debug, warn};

use super::super::{ToolingError, ToolingManager};

impl ToolingManager {
    #[allow(dead_code)]
    pub(crate) async fn link_memory_to_session(
        &self,
        memory_id: &str,
        session_id: &str,
        sequence: i64,
    ) -> Result<(), ToolingError> {
        let params = serde_json::json!({
            "memory_id": memory_id,
            "session_id": session_id,
            "sequence": sequence
        });
        self.db
            .execute_query::<serde_json::Value, _>("linkMemoryToSession", &params)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        debug!("Linked memory {} to session {}", memory_id, session_id);
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) async fn link_agent_to_memory(
        &self,
        agent_id: &str,
        memory_id: &str,
        method: &str,
    ) -> Result<(), ToolingError> {
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
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        debug!(
            "Linked agent {} to memory {} (method={})",
            agent_id, memory_id, method
        );
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) async fn add_entity_relation(
        &self,
        from_entity_id: &str,
        to_entity_id: &str,
        relationship_type: &str,
        strength: i64,
    ) -> Result<(), ToolingError> {
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
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        debug!(
            "Entity relation: {} -[{}]-> {}",
            from_entity_id, relationship_type, to_entity_id
        );
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) async fn add_entity_part_of(
        &self,
        part_id: &str,
        whole_id: &str,
    ) -> Result<(), ToolingError> {
        let params = serde_json::json!({
            "part_id": part_id,
            "whole_id": whole_id
        });
        self.db
            .execute_query::<serde_json::Value, _>("addEntityPartOf", &params)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        debug!("Entity part-of: {} is part of {}", part_id, whole_id);
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) async fn add_memory_valid_in_context(
        &self,
        memory_id: &str,
        context_id: &str,
        priority: i64,
    ) -> Result<(), ToolingError> {
        let params = serde_json::json!({
            "memory_id": memory_id,
            "context_id": context_id,
            "priority": priority,
            "exclusive": 0
        });
        self.db
            .execute_query::<serde_json::Value, _>("addMemoryValidIn", &params)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        debug!(
            "Memory {} valid in context {} (priority={})",
            memory_id, context_id, priority
        );
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) async fn add_concept_is_a(
        &self,
        child_id: &str,
        parent_id: &str,
        inheritance_type: &str,
    ) -> Result<(), ToolingError> {
        let params = serde_json::json!({
            "child_id": child_id,
            "parent_id": parent_id,
            "inheritance_type": inheritance_type
        });
        self.db
            .execute_query::<serde_json::Value, _>("addConceptIsA", &params)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        debug!("Concept IS_A: {} -> {}", child_id, parent_id);
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) async fn add_concept_relation(
        &self,
        from_id: &str,
        to_id: &str,
        relation_type: &str,
    ) -> Result<(), ToolingError> {
        let params = serde_json::json!({
            "from_id": from_id,
            "to_id": to_id,
            "relation_type": relation_type
        });
        self.db
            .execute_query::<serde_json::Value, _>("addConceptRelation", &params)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        debug!(
            "Concept relation: {} -[{}]-> {}",
            from_id, relation_type, to_id
        );
        Ok(())
    }

    #[allow(dead_code)]
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

        if let Err(e) = self
            .db
            .execute_query::<serde_json::Value, _>(
                "addMemoryContradiction",
                &ContradictInput {
                    from_id: new_memory_id.to_string(),
                    to_id: existing_memory_id.to_string(),
                    resolution: reasoning.to_string(),
                    resolved: 0,
                    resolution_strategy: format!("cross_user_{}", conflict_type),
                },
            )
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
