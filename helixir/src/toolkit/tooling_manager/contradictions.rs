//! Contradiction primitives (#45) — the "hands" the Atropos reconcile pass
//! composes over. These only read/retire CONTRADICTS edges; the drain POLICY
//! (preference vs factual, decay) lives in the agent (`agents::atropos::
//! reconcile`), per the agents→toolkit layering.

use serde::Deserialize;

use super::ToolingManager;
use super::types::ToolingError;
use crate::utils::nullable_string;

/// One open (`resolved=0`) outgoing dispute from a memory, as stored on the edge.
#[derive(Debug, Clone)]
pub struct OpenContradiction {
    pub from_id: String,
    pub to_id: String,
    pub resolution_strategy: String,
}

impl ToolingManager {
    /// All open outgoing contradictions across a user's memories — the worklist
    /// the Cutter drains. Drives from the member side (no global edge scan):
    /// for each memory, zip its `OutE<CONTRADICTS>` with its `Out<CONTRADICTS>`
    /// targets (parallel order) and keep the unresolved ones.
    pub async fn gather_open_contradictions(
        &self,
        user_id: &str,
        limit: i64,
    ) -> Result<Vec<OpenContradiction>, ToolingError> {
        #[derive(Deserialize, Default)]
        struct Full {
            #[serde(default)]
            out_edges: Vec<EdgeRow>,
            #[serde(default)]
            out_targets: Vec<TargetRow>,
        }
        #[derive(Deserialize, Default)]
        struct EdgeRow {
            #[serde(default)]
            resolved: i64,
            #[serde(default, deserialize_with = "nullable_string")]
            resolution_strategy: String,
        }
        #[derive(Deserialize, Default)]
        struct TargetRow {
            #[serde(default, deserialize_with = "nullable_string")]
            memory_id: String,
        }

        let memories = self.list_user_memories(user_id, limit).await?;
        let mut open = Vec::new();
        for (memory_id, _content) in memories {
            let resp: Full = self
                .db
                .execute_query(
                    "getMemoryContradictionsFull",
                    &serde_json::json!({ "memory_id": memory_id }),
                )
                .await
                .unwrap_or_default();
            for (e, t) in resp.out_edges.iter().zip(resp.out_targets.iter()) {
                if e.resolved == 0 && !t.memory_id.is_empty() {
                    open.push(OpenContradiction {
                        from_id: memory_id.clone(),
                        to_id: t.memory_id.clone(),
                        resolution_strategy: e.resolution_strategy.clone(),
                    });
                }
            }
        }
        Ok(open)
    }

    /// Record a `resolved=0` CONTRADICTS edge between two memories with a
    /// strategy label. The live cross-user path writes the same edge; exposed
    /// here for seeding/repair and deterministic reconcile testing.
    pub async fn record_contradiction(
        &self,
        from_id: &str,
        to_id: &str,
        strategy: &str,
    ) -> Result<(), ToolingError> {
        self.db
            .execute_query::<serde_json::Value, _>(
                "addMemoryContradiction",
                &serde_json::json!({
                    "from_id": from_id,
                    "to_id": to_id,
                    "resolution": "seeded",
                    "resolved": 0,
                    "resolution_strategy": strategy,
                }),
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        Ok(())
    }

    /// Retire every open outgoing contradiction from `memory_id`, stamping the
    /// drain `strategy` (e.g. `coexist_preference`). Non-destructive: the edge
    /// stays, only `resolved` flips to 1 — the dispute's history is preserved.
    pub async fn resolve_memory_contradictions(
        &self,
        memory_id: &str,
        strategy: &str,
    ) -> Result<(), ToolingError> {
        self.db
            .execute_query::<serde_json::Value, _>(
                "resolveMemoryContradictions",
                &serde_json::json!({ "memory_id": memory_id, "strategy": strategy }),
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        Ok(())
    }
}
