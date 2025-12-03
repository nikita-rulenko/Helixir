use chrono::Utc;
use helix_rs::HelixDB;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, warn};

#[derive(Error, Debug)]
pub enum RelationError {
    #[error("Database error: {0}")]
    Database(#[from] helix_rs::HelixError),
    #[error("Invalid link type: {0}")]
    InvalidLinkType(String),
    #[error("Missing required field: {0}")]
    MissingField(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RelationEdge {
    pub to: HashMap<String, serde_json::Value>,
    pub probability: Option<i32>,
    pub strength: Option<i32>,
    pub relation_type: Option<String>,
}

pub struct RelationCopier {
    client: HelixDB,
}

impl RelationCopier {
    pub fn new(client: HelixDB) -> Self {
        Self { client }
    }

    pub async fn copy_reasoning_relations(&self, old_memory_id: &str, new_memory_id: &str) -> Result<usize, RelationError> {
        let mut copied_count = 0;

        let result = self.client.execute_query("getMemoryOutgoingRelations", HashMap::from([("memory_id", old_memory_id)])).await?;
        
        let outgoing: HashMap<String, Vec<RelationEdge>> = serde_json::from_value(result)?;
        
        
        for edge in outgoing.get("implies_out").unwrap_or(&vec![]) {
            if let Some(target) = edge.to.get("memory_id").and_then(|v| v.as_str()) {
                if let Err(e) = self.client.execute_query("addMemoryImplication", HashMap::from([
                    ("from_id", new_memory_id),
                    ("to_id", target),
                    ("probability", &edge.probability.unwrap_or(80).to_string()),
                    ("reasoning_id", &format!("copied_from_{}", old_memory_id)),
                ])).await {
                    warn!("Failed to copy IMPLIES relation: {}", e);
                } else {
                    copied_count += 1;
                }
            }
        }

        
        for edge in outgoing.get("because_out").unwrap_or(&vec![]) {
            if let Some(target) = edge.to.get("memory_id").and_then(|v| v.as_str()) {
                if let Err(e) = self.client.execute_query("addMemoryCausation", HashMap::from([
                    ("from_id", new_memory_id),
                    ("to_id", target),
                    ("strength", &edge.strength.unwrap_or(80).to_string()),
                    ("reasoning_id", &format!("copied_from_{}", old_memory_id)),
                ])).await {
                    warn!("Failed to copy BECAUSE relation: {}", e);
                } else {
                    copied_count += 1;
                }
            }
        }

        
        for edge in outgoing.get("relations_out").unwrap_or(&vec![]) {
            if let Some(target) = edge.to.get("memory_id").and_then(|v| v.as_str()) {
                let metadata = format!(r#"{{"copied_from": "{}"}}"#, old_memory_id);
                if let Err(e) = self.client.execute_query("addMemoryRelation", HashMap::from([
                    ("source_id", new_memory_id),
                    ("target_id", target),
                    ("relation_type", edge.relation_type.as_deref().unwrap_or("related")),
                    ("strength", &edge.strength.unwrap_or(50).to_string()),
                    ("created_at", &Utc::now().to_rfc3339()),
                    ("metadata", &metadata),
                ])).await {
                    warn!("Failed to copy MEMORY_RELATION: {}", e);
                } else {
                    copied_count += 1;
                }
            }
        }

        debug!("Copied {} reasoning relations from {} to {}", copied_count, old_memory_id, new_memory_id);
        Ok(copied_count)
    }
}

pub struct MemoryRelationManager {
    client: HelixDB,
}

impl MemoryRelationManager {
    pub fn new(client: HelixDB) -> Self {
        Self { client }
    }

    pub async fn add_relation(
        &self,
        source_id: &str,
        target_id: &str,
        relation_type: &str,
        strength: i32,
        metadata: Option<&str>,
    ) -> Result<(), RelationError> {
        let metadata = metadata.unwrap_or("{}");
        self.client.execute_query("addMemoryRelation", HashMap::from([
            ("source_id", source_id),
            ("target_id", target_id),
            ("relation_type", relation_type),
            ("strength", &strength.to_string()),
            ("created_at", &Utc::now().to_rfc3339()),
            ("metadata", metadata),
        ])).await?;
        
        debug!("Added relation: {} -> {}", &source_id[..8], &target_id[..8]);
        Ok(())
    }

    pub async fn link_to_concept(
        &self,
        memory_id: &str,
        concept_id: &str,
        confidence: i32,
        link_type: &str,
    ) -> Result<(), RelationError> {
        match link_type {
            "INSTANCE_OF" => {
                self.client.execute_query("linkMemoryToInstanceOf", HashMap::from([
                    ("memory_id", memory_id),
                    ("concept_id", concept_id),
                    ("confidence", &confidence.to_string()),
                ])).await?;
            }
            "BELONGS_TO_CATEGORY" => {
                self.client.execute_query("linkMemoryToCategory", HashMap::from([
                    ("memory_id", memory_id),
                    ("concept_id", concept_id),
                    ("relevance", &confidence.to_string()),
                ])).await?;
            }
            _ => return Err(RelationError::InvalidLinkType(link_type.to_string())),
        }

        debug!("Linked memory {} to concept {}", crate::safe_truncate(memory_id, 8), concept_id);
        Ok(())
    }
}