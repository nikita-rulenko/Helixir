//! Cross-domain consolidation (Clotho-lite, #33).
//!
//! A background pass — deliberately NOT on the hot write path — that weaves
//! reasoning edges between memories that share an entity but are embedding-
//! dissimilar (the cross-domain bridges similarity-only candidate selection can
//! never surface). It runs over the SETTLED graph, so it sidesteps the write-
//! time ordering and snapshot-visibility pitfalls that bite an inline approach.
//!
//! Per the vision: the per-write path does the concrete, fast work; the
//! speculative cross-domain weaving is the mediator "dreaming" in the
//! background. Manual trigger now; it becomes the Clotho daemon loop later (#42).

use serde::Deserialize;
use tracing::{info, warn};

use super::ToolingManager;
use super::types::ToolingError;
use crate::utils::nullable_string;

#[derive(Debug, Default)]
pub struct ConsolidateStats {
    pub memories_scanned: usize,
    pub bridges_woven: usize,
}

#[derive(Deserialize, Default)]
struct MemoriesResp {
    #[serde(default)]
    memories: Vec<MemRow>,
}

#[derive(Deserialize, Clone)]
struct MemRow {
    #[serde(default, deserialize_with = "nullable_string")]
    memory_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    content: String,
}

#[derive(Deserialize, Default)]
struct EntitiesResp {
    #[serde(default)]
    entities: Vec<EntRow>,
}

#[derive(Deserialize)]
struct EntRow {
    #[serde(default, deserialize_with = "nullable_string")]
    entity_id: String,
}

impl ToolingManager {
    /// Weave cross-domain reasoning edges for a user's memories via shared
    /// entities. `max_memories` caps the scan; `per_memory` caps candidates per
    /// memory (bounds the LLM inference cost). Returns how many bridges were
    /// woven. Idempotent enough for repeated runs — inference re-proposing an
    /// existing relation just re-asserts the edge.
    pub async fn consolidate_user(
        &self,
        user_id: &str,
        max_memories: i64,
        per_memory: i64,
    ) -> Result<ConsolidateStats, ToolingError> {
        let mut stats = ConsolidateStats::default();

        let resp: MemoriesResp = self
            .db
            .execute_query(
                "getUserMemories",
                &serde_json::json!({ "user_id": user_id, "limit": max_memories }),
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        for m in &resp.memories {
            if m.memory_id.is_empty() {
                continue;
            }
            stats.memories_scanned += 1;

            // Entities this memory extracted.
            let ents: EntitiesResp = match self
                .db
                .execute_query(
                    "getMemoryEntities",
                    &serde_json::json!({ "memory_id": m.memory_id }),
                )
                .await
            {
                Ok(e) => e,
                Err(e) => {
                    warn!(
                        "consolidate: getMemoryEntities failed for {}: {}",
                        m.memory_id, e
                    );
                    continue;
                }
            };

            // Memories sharing any of those entities (cross-domain candidates).
            let mut seen = std::collections::HashSet::new();
            seen.insert(m.memory_id.clone());
            let mut candidates: Vec<(String, String)> = Vec::new();
            for ent in &ents.entities {
                if ent.entity_id.is_empty() {
                    continue;
                }
                match self
                    .fetch_memories_by_entity(&ent.entity_id, &m.memory_id, per_memory)
                    .await
                {
                    Ok(co) => {
                        for (id, content) in co {
                            if seen.insert(id.clone()) {
                                candidates.push((id, content));
                            }
                        }
                    }
                    Err(e) => warn!("consolidate: getMemoriesByEntity failed: {}", e),
                }
            }
            if candidates.is_empty() {
                continue;
            }
            candidates.truncate(per_memory.max(0) as usize);

            // Infer typed relations between this memory and its cross-domain
            // candidates, then persist them.
            match self
                .reasoning_engine
                .infer_relations(&m.memory_id, &m.content, &candidates)
                .await
            {
                Ok(inferred) => {
                    for rel in &inferred {
                        match self
                            .reasoning_engine
                            .add_relation(
                                &rel.from_memory_id,
                                &rel.to_memory_id,
                                rel.relation_type,
                                rel.strength,
                                rel.reasoning_id.as_deref(),
                            )
                            .await
                        {
                            Ok(_) => stats.bridges_woven += 1,
                            Err(e) => warn!("consolidate: add_relation failed: {}", e),
                        }
                    }
                }
                Err(e) => warn!(
                    "consolidate: infer_relations failed for {}: {}",
                    m.memory_id, e
                ),
            }
        }

        info!(
            "consolidate_user({}): scanned={}, bridges_woven={}",
            user_id, stats.memories_scanned, stats.bridges_woven
        );
        Ok(stats)
    }

    async fn fetch_memories_by_entity(
        &self,
        entity_id: &str,
        exclude_memory_id: &str,
        limit: i64,
    ) -> Result<Vec<(String, String)>, ToolingError> {
        let resp: MemoriesResp = self
            .db
            .execute_query(
                "getMemoriesByEntity",
                &serde_json::json!({
                    "entity_id": entity_id,
                    "exclude_memory_id": exclude_memory_id,
                    "limit": limit
                }),
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        Ok(resp
            .memories
            .into_iter()
            .filter(|m| !m.memory_id.is_empty())
            .map(|m| (m.memory_id, m.content))
            .collect())
    }
}
