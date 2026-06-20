//! Phase 2 — cross-user deduplication. `apply_cross_user_phase` spawns a
//! background task that re-asks the decision engine against the global
//! (collective) search results. Three observable outcomes:
//! - `LinkExisting` → attach the current user to the existing memory via
//!   `HAS_MEMORY` and bump `user_count` (Hive linkage).
//! - `CrossContradict` → create a `MemoryContradiction` edge.
//! - `Noop` → if the very same content exists already, link silently.
//! - anything else → no cross-user action.
//!
//! The two free functions below run inside the spawned task (no `Self`).

use serde::Serialize;
use tracing::{debug, info, warn};

use crate::llm::decision::{MemoryOperation, SimilarMemory};
use crate::llm::extractor::ExtractedMemory;

use super::super::{ToolingError, ToolingManager};

impl ToolingManager {
    pub(super) async fn apply_cross_user_phase(
        &self,
        memory: &ExtractedMemory,
        user_id: &str,
        vector: &[f32],
        new_memory_id: &str,
        _relations_created: &mut usize,
    ) -> Result<(), ToolingError> {
        info!(
            "Phase 2: cross-user dedup for {} (user={})",
            new_memory_id, user_id
        );
        let global_results = self
            .search_engine
            .search_for_dedup(&memory.text, vector, user_id, self.config.write.cross_user_dedup_top_k)
            .await
            .unwrap_or_default();

        let cross_user_similar: Vec<SimilarMemory> = global_results
            .iter()
            .filter(|r| {
                let result_user = r
                    .metadata
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                !result_user.is_empty() && result_user != user_id && r.memory_id != new_memory_id
            })
            .map(|r| SimilarMemory {
                id: r.memory_id.clone(),
                content: r.content.clone(),
                score: r.score as f64,
                created_at: Some(r.created_at.clone()),
                user_id: r
                    .metadata
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                is_cross_user: true,
                memory_type: r
                    .metadata
                    .get("memory_type")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
            })
            .collect();

        if cross_user_similar.is_empty() {
            debug!("Phase 2: no cross-user candidates found");
            return Ok(());
        }

        info!(
            "Phase 2: {} cross-user candidates, spawning background LLM decision",
            cross_user_similar.len()
        );

        let memory_text = memory.text.clone();
        let memory_type = memory.memory_type.clone();
        let user_id_owned = user_id.to_string();
        let new_mem_id = new_memory_id.to_string();
        let db = self.db.clone();
        let decision_engine = self.decision_engine.clone();

        tokio::spawn(async move {
            let cross_decision = decision_engine
                .decide(
                    &memory_text,
                    &memory_type,
                    &cross_user_similar,
                    &user_id_owned,
                )
                .await;
            info!(
                "Phase 2 bg: LLM decided {:?} (confidence={})",
                cross_decision.operation, cross_decision.confidence
            );

            match cross_decision.operation {
                MemoryOperation::LinkExisting => {
                    // #43: duplicate consolidation is now handled by the content_key
                    // fingerprint group — the writer already has their own personal
                    // node carrying the shared fingerprint, and collective user_count
                    // counts the group. Cross-linking the writer onto another user's
                    // node here would double-count the group AND reopen the snapshot-
                    // lag race this fix removes. So a cross-user duplicate is a no-op.
                    if let Some(link_id) = &cross_decision.link_to_memory_id {
                        debug!(
                            "Phase 2 bg: LINK_EXISTING {} → {} is a no-op (fingerprint group handles consensus)",
                            user_id_owned, link_id
                        );
                    }
                }
                MemoryOperation::CrossContradict => {
                    if let Some(contra_id) = &cross_decision.contradicts_memory_id {
                        info!(
                            "Phase 2 bg: CROSS_CONTRADICT {} ↔ {}",
                            new_mem_id, contra_id
                        );
                        add_contradiction_bg(
                            &db,
                            &new_mem_id,
                            contra_id,
                            cross_decision
                                .conflict_type
                                .as_deref()
                                .unwrap_or("preference"),
                            &cross_decision.reasoning,
                        )
                        .await;
                        // Cognitive layer: the user now has a RELATION to the
                        // other user's fact — they dispute it.
                        link_user_to_memory_with_stance_bg(
                            &db,
                            &user_id_owned,
                            contra_id,
                            "disputes",
                            cross_decision.confidence as i64,
                        )
                        .await;
                    }
                }
                MemoryOperation::Noop => {
                    // #43: same as LinkExisting — the fingerprint group already
                    // captures this writer as a holder via their own node; no
                    // cross-link (it would double-count and race).
                    if let Some(existing) = cross_user_similar.first() {
                        debug!(
                            "Phase 2 bg: NOOP for {} vs {} (fingerprint group handles consensus)",
                            user_id_owned, existing.id
                        );
                    }
                }
                _ => {
                    debug!("Phase 2 bg: no cross-user action needed");
                }
            }
        });

        Ok(())
    }
}

/// Cognitive layer (#33): the HAS_MEMORY edge carries the user's RELATION to
/// the shared fact, written from the Phase-2 verdict that is already being
/// computed — no extra LLM calls.
async fn link_user_to_memory_with_stance_bg(
    db: &crate::db::HelixClient,
    user_id: &str,
    memory_id: &str,
    stance: &str,
    certainty: i64,
) {
    #[derive(Serialize)]
    struct EnsureUser {
        user_id: String,
        name: String,
    }
    let _ = db
        .execute_query::<serde_json::Value, _>("getUser", &serde_json::json!({"user_id": user_id}))
        .await
        .or_else(|_| {
            futures::executor::block_on(async {
                db.execute_query::<serde_json::Value, _>(
                    "addUser",
                    &EnsureUser {
                        user_id: user_id.to_string(),
                        name: user_id.to_string(),
                    },
                )
                .await
            })
        });

    #[derive(Serialize)]
    struct LinkInput {
        user_id: String,
        memory_id: String,
        context: String,
        stance: String,
        certainty: i64,
        linked_at: String,
    }
    if let Err(e) = db
        .execute_query::<serde_json::Value, _>(
            "linkUserToMemoryWithStance",
            &LinkInput {
                user_id: user_id.to_string(),
                memory_id: memory_id.to_string(),
                context: "cross_user_link".to_string(),
                stance: stance.to_string(),
                certainty,
                linked_at: chrono::Utc::now().to_rfc3339(),
            },
        )
        .await
    {
        warn!(
            "Phase 2 bg: failed to link user {} to memory {}: {}",
            user_id, memory_id, e
        );
        return;
    }

    #[derive(serde::Deserialize)]
    struct UsersResult {
        #[serde(default)]
        users: Vec<serde_json::Value>,
    }
    let user_count = match db
        .execute_query::<UsersResult, _>(
            "getMemoryUsers",
            &serde_json::json!({"memory_id": memory_id}),
        )
        .await
    {
        Ok(r) => r.users.len().max(1) as i64,
        Err(_) => 2,
    };

    #[derive(Serialize)]
    struct UpdateCount {
        memory_id: String,
        user_count: i64,
        updated_at: String,
    }
    let _ = db
        .execute_query::<serde_json::Value, _>(
            "updateMemoryUserCount",
            &UpdateCount {
                memory_id: memory_id.to_string(),
                user_count,
                updated_at: chrono::Utc::now().to_rfc3339(),
            },
        )
        .await;

    info!(
        "Phase 2 bg: linked user {} to memory {} (user_count={})",
        user_id, memory_id, user_count
    );
}

async fn add_contradiction_bg(
    db: &crate::db::HelixClient,
    from_id: &str,
    to_id: &str,
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
    if let Err(e) = db
        .execute_query::<serde_json::Value, _>(
            "addMemoryContradiction",
            &ContradictInput {
                from_id: from_id.to_string(),
                to_id: to_id.to_string(),
                resolution: reasoning.to_string(),
                resolved: 0,
                resolution_strategy: format!("cross_user_{}", conflict_type),
            },
        )
        .await
    {
        warn!(
            "Phase 2 bg: failed to add contradiction {} → {}: {}",
            from_id, to_id, e
        );
    } else {
        info!(
            "Phase 2 bg: added cross-user contradiction {} → {}",
            from_id, to_id
        );
    }
}
