//! Entity → entity relation persistence: given two `ExtractedEntity`s and the
//! LLM-suggested relationship_type/strength, resolve both sides into DB
//! entities and write the `addEntityRelation` edge.

use serde::Serialize;
use tracing::{info, warn};

use crate::llm::extractor::ExtractedEntity;

use super::super::ToolingManager;

impl ToolingManager {
    pub(super) async fn persist_entity_relation(
        &self,
        from_entity_id: &str,
        target_entity_id: &str,
        relationship_type: &str,
        strength: i64,
        extraction_entities: &[ExtractedEntity],
    ) {
        let from_entity = extraction_entities.iter().find(|e| e.id == from_entity_id);
        let to_entity = extraction_entities
            .iter()
            .find(|e| e.id == target_entity_id);

        let (from_name, from_type) = match from_entity {
            Some(e) => (e.name.as_str(), e.entity_type.as_str()),
            None => return,
        };
        let (to_name, to_type) = match to_entity {
            Some(e) => (e.name.as_str(), e.entity_type.as_str()),
            None => return,
        };

        let from_db = match self
            .entity_manager
            .get_or_create_entity(from_name, from_type, None)
            .await
        {
            Ok(e) => e,
            Err(e) => {
                warn!(
                    "Failed to resolve entity '{}' for relation: {}",
                    from_name, e
                );
                return;
            }
        };
        let to_db = match self
            .entity_manager
            .get_or_create_entity(to_name, to_type, None)
            .await
        {
            Ok(e) => e,
            Err(e) => {
                warn!("Failed to resolve entity '{}' for relation: {}", to_name, e);
                return;
            }
        };

        #[derive(Serialize)]
        struct EntityRelationParams {
            from_id: String,
            to_id: String,
            relationship_type: String,
            strength: i64,
            bidirectional: i64,
        }

        let params = EntityRelationParams {
            from_id: from_db.entity_id.clone(),
            to_id: to_db.entity_id.clone(),
            relationship_type: relationship_type.to_string(),
            strength,
            bidirectional: 0,
        };

        match self
            .db
            .execute_query::<serde_json::Value, _>("addEntityRelation", &params)
            .await
        {
            Ok(_) => {
                info!(
                    "Created entity relation: {} -[{}]-> {}",
                    from_name, relationship_type, to_name
                );
            }
            Err(e) => {
                warn!(
                    "Failed to create entity relation {} -> {}: {}",
                    from_name, to_name, e
                );
            }
        }
    }
}
