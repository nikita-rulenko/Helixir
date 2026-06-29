//! Reasoning-edge CRUD: [`ReasoningEngine::add_relation`] and the
//! private [`ReasoningEngine::edge_exists`] dedup check.

use serde::Deserialize;
use tracing::{debug, warn};

use super::engine::ReasoningEngine;
use super::types::{ReasoningError, ReasoningRelation, ReasoningType};

impl ReasoningEngine {
    pub async fn add_relation(
        &self,
        from_id: &str,
        to_id: &str,
        relation_type: ReasoningType,
        strength: i32,
        reasoning_id: Option<&str>,
    ) -> Result<ReasoningRelation, ReasoningError> {
        let strength = strength.clamp(0, 100);

        // Reject self-referential reasoning edges. A memory cannot logically
        // IMPLIES / BECAUSE / CONTRADICTS / SUPPORTS itself; persisting such
        // an edge corrupts later chain traversal. See issue #16.
        if from_id == to_id {
            warn!(
                "Rejecting self-referential {} edge for memory {}",
                relation_type.edge_name(),
                crate::safe_truncate(from_id, 12)
            );
            return Err(ReasoningError::Invalid(format!(
                "self-referential {} edge on {}",
                relation_type.edge_name(),
                from_id
            )));
        }

        if self.edge_exists(from_id, to_id, relation_type).await {
            debug!(
                "Skipping duplicate {} edge: {} -> {}",
                relation_type.edge_name(),
                crate::safe_truncate(from_id, 12),
                crate::safe_truncate(to_id, 12)
            );
            return Ok(ReasoningRelation {
                peer_memory_id: String::new(),
                peer_memory_content: String::new(),
                relation_id: format!(
                    "rel_{}_{}",
                    crate::safe_truncate(from_id, 8),
                    crate::safe_truncate(to_id, 8)
                ),
                from_memory_id: from_id.to_string(),
                to_memory_id: to_id.to_string(),
                to_memory_content: String::new(),
                from_memory_content: String::new(),
                relation_type,
                strength,
                reasoning_id: reasoning_id.map(String::from),
            });
        }

        let relation = ReasoningRelation {
            peer_memory_id: String::new(),
            peer_memory_content: String::new(),
            relation_id: format!(
                "rel_{}_{}",
                crate::safe_truncate(from_id, 8),
                crate::safe_truncate(to_id, 8)
            ),
            from_memory_id: from_id.to_string(),
            to_memory_id: to_id.to_string(),
            to_memory_content: String::new(),
            from_memory_content: String::new(),
            relation_type,
            strength,
            reasoning_id: reasoning_id.map(String::from),
        };

        #[derive(Deserialize)]
        #[allow(dead_code)] // HelixDB edge-creation ack envelope.
        struct EdgeResponse {
            #[serde(default)]
            edge: serde_json::Value,
        }

        let persist_result = match relation_type {
            ReasoningType::Implies => {
                self.client
                    .execute_query::<EdgeResponse, _>(
                        "addMemoryImplication",
                        &serde_json::json!({
                            "from_id": from_id,
                            "to_id": to_id,
                            "probability": strength as i64,
                            "reasoning_id": reasoning_id.unwrap_or(""),
                        }),
                    )
                    .await
            }
            ReasoningType::Because => {
                self.client
                    .execute_query::<EdgeResponse, _>(
                        "addMemoryCausation",
                        &serde_json::json!({
                            "from_id": from_id,
                            "to_id": to_id,
                            "strength": strength as i64,
                            "reasoning_id": reasoning_id.unwrap_or(""),
                        }),
                    )
                    .await
            }
            ReasoningType::Contradicts => {
                self.client
                    .execute_query::<EdgeResponse, _>(
                        "addMemoryContradiction",
                        &serde_json::json!({
                            "from_id": from_id,
                            "to_id": to_id,
                            "resolution": "",
                            "resolved": 0i64,
                            "resolution_strategy": "pending",
                        }),
                    )
                    .await
            }
            // SUPPORTS + the associative arsenal (RELATES_TO / PART_OF / IS_A)
            // all persist via the generic MEMORY_RELATION edge, tagged by their
            // edge_name() — no dedicated query / schema change needed.
            ReasoningType::Supports
            | ReasoningType::RelatesTo
            | ReasoningType::PartOf
            | ReasoningType::IsA => {
                let now = chrono::Utc::now().to_rfc3339();
                self.client
                    .execute_query::<EdgeResponse, _>(
                        "addReasoningRelation",
                        &serde_json::json!({
                            "relation_id": format!("rel_{}_{}", crate::safe_truncate(from_id, 8), crate::safe_truncate(to_id, 8)),
                            "from_memory_id": from_id,
                            "to_memory_id": to_id,
                            "relation_type": relation_type.edge_name(),
                            "strength": strength as i64,
                            "confidence": 80i64,
                            "explanation": "",
                            "created_by": "reasoning_engine",
                            "created_at": now,
                        }),
                    )
                    .await
            }
        };

        persist_result.map_err(|e| ReasoningError::Database(e.to_string()))?;

        self.relation_cache
            .lock()
            .put(relation.relation_id.clone(), relation.clone());

        debug!(
            "Added {} relation: {} -> {} (strength={})",
            relation_type.edge_name(),
            from_id,
            to_id,
            strength
        );

        Ok(relation)
    }

    pub(super) async fn edge_exists(
        &self,
        from_id: &str,
        to_id: &str,
        relation_type: ReasoningType,
    ) -> bool {
        #[derive(Deserialize)]
        struct ConnectionsResult {
            #[serde(default)]
            implies_out: Vec<MemNode>,
            #[serde(default)]
            because_out: Vec<MemNode>,
            #[serde(default)]
            contradicts_out: Vec<MemNode>,
            #[serde(default)]
            relation_out: Vec<MemNode>,
        }

        #[derive(Deserialize)]
        struct MemNode {
            #[serde(default)]
            memory_id: String,
        }

        let result = match self
            .client
            .execute_query::<ConnectionsResult, _>(
                "getMemoryLogicalConnections",
                &serde_json::json!({"memory_id": from_id}),
            )
            .await
        {
            Ok(r) => r,
            Err(_) => return false,
        };

        let targets = match relation_type {
            ReasoningType::Implies => &result.implies_out,
            ReasoningType::Because => &result.because_out,
            ReasoningType::Contradicts => &result.contradicts_out,
            // SUPPORTS + associative edges all live on the generic relation bucket.
            ReasoningType::Supports
            | ReasoningType::RelatesTo
            | ReasoningType::PartOf
            | ReasoningType::IsA => &result.relation_out,
        };

        targets.iter().any(|n| n.memory_id == to_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn add_relation_rejects_self_loop() {
        let client = Arc::new(crate::db::HelixClient::new("127.0.0.1", 1).unwrap());
        let engine = ReasoningEngine::new(client, None, 16);

        // Guard rejects self-loops before any DB roundtrip; see issue #16.
        for rt in [
            ReasoningType::Implies,
            ReasoningType::Because,
            ReasoningType::Contradicts,
            ReasoningType::Supports,
        ] {
            let result = engine.add_relation("mem_x", "mem_x", rt, 80, None).await;
            assert!(
                matches!(result, Err(ReasoningError::Invalid(_))),
                "self-loop must be rejected for {:?}",
                rt
            );
        }
    }
}
