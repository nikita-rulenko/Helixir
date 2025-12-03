use crate::db::HelixClient;
use std::sync::Arc;
use chrono::Utc;
use super::models::{MemoryRelation, RelationType};
use tracing::{debug, warn};
use thiserror::Error;
use serde::Serialize;

#[derive(Error, Debug)]
pub enum EdgeCreatorError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Invalid relation type: {0}")]
    InvalidRelationType(String),
}

pub struct EdgeCreator {
    client: Arc<HelixClient>,
}

impl EdgeCreator {
    pub fn new(client: Arc<HelixClient>) -> Self {
        Self { client }
    }

    pub async fn create_relations(
        &self,
        source_id: &str,
        relations: &[MemoryRelation],
    ) -> Result<usize, EdgeCreatorError> {
        if relations.is_empty() {
            return Ok(0);
        }

        let mut created = 0;

        for rel in relations {
            let result: Result<(), _> = match rel.relation_type {
                RelationType::Implies => {
                    #[derive(Serialize)]
                    struct Params {
                        from_id: String,
                        to_id: String,
                        probability: i32,
                        reasoning_id: String,
                    }

                    self.client
                        .execute_query(
                            "addMemoryImplication",
                            &Params {
                                from_id: source_id.to_string(),
                                to_id: rel.target_id.clone(),
                                probability: (rel.confidence * 100.0) as i32,
                                reasoning_id: crate::safe_truncate(&rel.reasoning, 255).to_string(),
                            },
                        )
                        .await
                }
                RelationType::Because => {
                    #[derive(Serialize)]
                    struct Params {
                        from_id: String,
                        to_id: String,
                        strength: i32,
                        reasoning_id: String,
                    }

                    self.client
                        .execute_query(
                            "addMemoryCausation",
                            &Params {
                                from_id: source_id.to_string(),
                                to_id: rel.target_id.clone(),
                                strength: (rel.confidence * 100.0) as i32,
                                reasoning_id: crate::safe_truncate(&rel.reasoning, 255).to_string(),
                            },
                        )
                        .await
                }
                RelationType::Contradicts => {
                    #[derive(Serialize)]
                    struct Params {
                        from_id: String,
                        to_id: String,
                        resolution: String,
                        resolved: i32,
                        resolution_strategy: String,
                    }

                    self.client
                        .execute_query(
                            "addMemoryContradiction",
                            &Params {
                                from_id: source_id.to_string(),
                                to_id: rel.target_id.clone(),
                                resolution: String::new(),
                                resolved: 0,
                                resolution_strategy: crate::safe_truncate(&rel.reasoning, 255).to_string(),
                            },
                        )
                        .await
                }
                RelationType::RelatesTo | RelationType::Supersedes => {
                    #[derive(Serialize)]
                    struct Params {
                        source_id: String,
                        target_id: String,
                        relation_type: String,
                        strength: i32,
                        created_at: String,
                        metadata: String,
                    }

                    self.client
                        .execute_query(
                            "addMemoryRelation",
                            &Params {
                                source_id: source_id.to_string(),
                                target_id: rel.target_id.clone(),
                                relation_type: format!("{:?}", rel.relation_type),
                                strength: (rel.confidence * 100.0) as i32,
                                created_at: Utc::now().to_rfc3339(),
                                metadata: rel.reasoning.clone(),
                            },
                        )
                        .await
                }
            };

            match result {
                Ok(_) => {
                    created += 1;
                    debug!(
                        "Created {:?} relation: {} -> {}",
                        rel.relation_type,
                        crate::safe_truncate(source_id, 8),
                        &rel.target_id[..8.min(rel.target_id.len())]
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to create relation {} -> {}: {}",
                        crate::safe_truncate(source_id, 8),
                        &rel.target_id[..8.min(rel.target_id.len())],
                        e
                    );
                }
            }
        }

        Ok(created)
    }
}