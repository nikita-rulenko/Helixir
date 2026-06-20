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

    /// Owners (user_ids) of a memory — who a dispute on it should surface to.
    pub async fn memory_owners(&self, memory_id: &str) -> Vec<String> {
        #[derive(Deserialize, Default)]
        struct Resp {
            #[serde(default)]
            users: Vec<UserRow>,
        }
        #[derive(Deserialize)]
        struct UserRow {
            #[serde(default, deserialize_with = "nullable_string")]
            user_id: String,
        }
        let resp: Resp = self
            .db
            .execute_query("getMemoryUsers", &serde_json::json!({ "memory_id": memory_id }))
            .await
            .unwrap_or_default();
        resp.users
            .into_iter()
            .map(|u| u.user_id)
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Surface a live dispute to the owners of `to_id` as an outbox question
    /// (#25/#39), unless an identical notice is already pending for that owner —
    /// the daemon reconciles every pass and must never spam. Returns how many
    /// owners were newly notified. Never resolves anything: the owner decides.
    pub async fn surface_dispute(
        &self,
        from_id: &str,
        to_id: &str,
        resolution_strategy: &str,
    ) -> usize {
        let owners = self.memory_owners(to_id).await;
        let mut notified = 0;
        for owner in owners {
            let notice_id = format!("cr_{from_id}_{to_id}_{owner}");
            if self.has_pending_notice(&owner, &notice_id).await {
                continue;
            }
            let payload = serde_json::json!({
                "from_id": from_id,
                "to_id": to_id,
                "resolution_strategy": resolution_strategy,
                "question": format!(
                    "A live cross-user dispute contradicts your memory {to_id} — \
                     reconcile (confirm / retract / mark as preference)?"
                ),
            })
            .to_string();
            let ok = self
                .db
                .execute_query::<serde_json::Value, _>(
                    "enqueueNotice",
                    &serde_json::json!({
                        "notice_id": notice_id,
                        "user_id": owner,
                        "kind": "contradiction_review",
                        "payload": payload,
                        "pending_id": "",
                        "created_at": chrono::Utc::now().to_rfc3339(),
                    }),
                )
                .await
                .is_ok();
            if ok {
                notified += 1;
            }
        }
        notified
    }

    async fn has_pending_notice(&self, user_id: &str, notice_id: &str) -> bool {
        #[derive(Deserialize, Default)]
        struct Resp {
            #[serde(default)]
            notices: Vec<NoticeRow>,
        }
        #[derive(Deserialize)]
        struct NoticeRow {
            #[serde(default, deserialize_with = "nullable_string")]
            notice_id: String,
        }
        let resp: Resp = self
            .db
            .execute_query(
                "getUndeliveredNotices",
                &serde_json::json!({ "user_id": user_id, "limit": 1000 }),
            )
            .await
            .unwrap_or_default();
        resp.notices.iter().any(|n| n.notice_id == notice_id)
    }

    /// True if `memory_id` has been superseded — something points a SUPERSEDES
    /// edge AT it (`In<SUPERSEDES>` non-empty). The temporal signal the drain
    /// policy uses to retire a moot factual dispute toward the live side (#45).
    pub async fn is_superseded(&self, memory_id: &str) -> bool {
        #[derive(Deserialize, Default)]
        struct Resp {
            #[serde(default)]
            superseding: Vec<serde_json::Value>,
        }
        let resp: Resp = self
            .db
            .execute_query(
                "getSupersedingMemory",
                &serde_json::json!({ "memory_id": memory_id }),
            )
            .await
            .unwrap_or_default();
        !resp.superseding.is_empty()
    }

    /// Record a SUPERSEDES edge (`new_id` supersedes `old_id`). Exposed for
    /// seeding/repair and deterministic testing of the temporal drain.
    pub async fn record_supersession(
        &self,
        new_id: &str,
        old_id: &str,
        reason: &str,
    ) -> Result<(), ToolingError> {
        self.db
            .execute_query::<serde_json::Value, _>(
                "addMemorySupersession",
                &serde_json::json!({
                    "new_id": new_id,
                    "old_id": old_id,
                    "reason": reason,
                    "superseded_at": chrono::Utc::now().to_rfc3339(),
                    "is_contradiction": 0,
                }),
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        Ok(())
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
