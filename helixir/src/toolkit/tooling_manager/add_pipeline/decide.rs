//! Phase 1 dispatch: turn a [`MemoryDecision`] into the right side-effect.
//!
//! `ADD` / `UPDATE` / `SUPERSEDE` / `CONTRADICT` / `DELETE` / `NOOP` are all
//! handled here. `LinkExisting` / `CrossContradict` are produced by Phase 2
//! only and are an `unreachable!` in this stage.

use serde::Serialize;
use tracing::{debug, warn};

use crate::llm::decision::{MemoryDecision, MemoryOperation, SimilarMemory};
use crate::llm::extractor::ExtractedMemory;
use crate::toolkit::mind_toolbox::reasoning::ReasoningType;

use super::super::{ToolingError, ToolingManager};

impl ToolingManager {
    #[allow(clippy::too_many_arguments)] // intentional: write-side fan-out into the orchestrator's accumulators.
    pub(super) async fn handle_memory_operation(
        &self,
        decision: &MemoryDecision,
        memory: &ExtractedMemory,
        user_id: &str,
        agent_id: Option<&str>,
        tags: &str,
        vector: &[f32],
        phase1_similar: &[SimilarMemory],
        added_ids: &mut Vec<String>,
        updated_ids: &mut Vec<String>,
        deduped_ids: &mut Vec<String>,
        skipped: &mut usize,
        chunks_created: &mut usize,
        relations_created: &mut usize,
    ) -> Result<Option<String>, ToolingError> {
        let memory_id = match decision.operation {
            MemoryOperation::Noop => {
                debug!("NOOP: duplicate memory");
                *skipped += 1;
                if let Some(target_id) = &decision.target_memory_id {
                    // #44: surface the existing memory the write deduped to, so the
                    // agent sees "linked to X" rather than an empty/silent result.
                    deduped_ids.push(target_id.clone());
                    self.emit_memory_deduplicated(target_id, user_id).await;
                }
                return Ok(None);
            }
            MemoryOperation::Update => {
                if let (Some(target_id), Some(merged)) =
                    (&decision.target_memory_id, &decision.merged_content)
                {
                    if !Self::is_coherent_memory(merged) {
                        warn!(
                            "UPDATE merged_content is incoherent, falling back to ADD: {}...",
                            &merged.chars().take(60).collect::<String>()
                        );
                        let (new_id, new_chunks) =
                            self.store_new_memory(memory, user_id, vector, tags).await?;
                        *chunks_created += new_chunks;
                        let _ = self
                            .add_memory_history_event(&new_id, "ADD", "", &memory.text, user_id)
                            .await;
                        added_ids.push(new_id.clone());
                        new_id
                    } else {
                        debug!("UPDATE: updating {} with merged content", target_id);
                        let old_content = phase1_similar
                            .iter()
                            .find(|m| m.id == *target_id)
                            .map(|m| m.content.as_str())
                            .unwrap_or("");
                        self.update_memory_internal(target_id, merged, vector)
                            .await?;
                        let _ = self
                            .add_memory_history_event(
                                target_id,
                                "UPDATE",
                                old_content,
                                merged,
                                user_id,
                            )
                            .await;
                        updated_ids.push(target_id.to_string());
                        self.emit_memory_updated(target_id, user_id).await;
                        target_id.to_string()
                    }
                } else {
                    let (new_id, new_chunks) =
                        self.store_new_memory(memory, user_id, vector, tags).await?;
                    *chunks_created += new_chunks;
                    let _ = self
                        .add_memory_history_event(&new_id, "ADD", "", &memory.text, user_id)
                        .await;
                    new_id
                }
            }
            MemoryOperation::Supersede => {
                let (new_id, new_chunks) =
                    self.store_new_memory(memory, user_id, vector, tags).await?;
                *chunks_created += new_chunks;
                let _ = self
                    .add_memory_history_event(&new_id, "ADD", "", &memory.text, user_id)
                    .await;
                if let Some(old_id) = &decision.supersedes_memory_id {
                    debug!("SUPERSEDE: {} supersedes {}", new_id, old_id);
                    #[derive(Serialize)]
                    struct SupersedeParams {
                        new_id: String,
                        old_id: String,
                        reason: String,
                        superseded_at: String,
                        is_contradiction: i64,
                    }
                    let _ = self
                        .db
                        .execute_query::<serde_json::Value, _>(
                            "addMemorySupersession",
                            &SupersedeParams {
                                new_id: new_id.clone(),
                                old_id: old_id.clone(),
                                reason: decision.reasoning.clone(),
                                superseded_at: chrono::Utc::now().to_rfc3339(),
                                is_contradiction: 0,
                            },
                        )
                        .await;
                    *relations_created += 1;
                    let _ = self
                        .add_memory_history_event(old_id, "SUPERSEDE", "", &new_id, user_id)
                        .await;
                    self.emit_memory_superseded(&new_id, old_id, user_id).await;
                }
                added_ids.push(new_id.clone());
                new_id
            }
            MemoryOperation::Contradict => {
                let (new_id, new_chunks) =
                    self.store_new_memory(memory, user_id, vector, tags).await?;
                *chunks_created += new_chunks;
                let _ = self
                    .add_memory_history_event(&new_id, "ADD", "", &memory.text, user_id)
                    .await;
                if let Some(contra_id) = &decision.contradicts_memory_id {
                    debug!("CONTRADICT: {} contradicts {}", new_id, contra_id);
                    let _ = self
                        .reasoning_engine
                        .add_relation(
                            &new_id,
                            contra_id,
                            ReasoningType::Contradicts,
                            self.config.write.contradict_edge_strength as i32,
                            None,
                        )
                        .await;
                }
                added_ids.push(new_id.clone());
                new_id
            }
            MemoryOperation::Delete => {
                // Elder-brain contract (#34, README "no deletion"): the engine
                // is not allowed to destroy a memory. A DELETE verdict is
                // executed as SUPERSEDE — the old fact stays in history,
                // reachable forever; the delete intent is preserved in the
                // supersession reason and the charter escalates it to the
                // agent via needs_clarification.
                let (new_id, new_chunks) =
                    self.store_new_memory(memory, user_id, vector, tags).await?;
                *chunks_created += new_chunks;
                let _ = self
                    .add_memory_history_event(&new_id, "ADD", "", &memory.text, user_id)
                    .await;
                if let Some(target_id) = &decision.target_memory_id {
                    tracing::warn!(
                        "DELETE verdict for {} blocked by charter C1 — executing as SUPERSEDE",
                        target_id
                    );
                    #[derive(Serialize)]
                    struct SupersedeParams {
                        new_id: String,
                        old_id: String,
                        reason: String,
                        superseded_at: String,
                        is_contradiction: i64,
                    }
                    let _ = self
                        .db
                        .execute_query::<serde_json::Value, _>(
                            "addMemorySupersession",
                            &SupersedeParams {
                                new_id: new_id.clone(),
                                old_id: target_id.clone(),
                                reason: format!(
                                    "delete-intent blocked by charter C1: {}",
                                    decision.reasoning
                                ),
                                superseded_at: chrono::Utc::now().to_rfc3339(),
                                is_contradiction: 0,
                            },
                        )
                        .await;
                    *relations_created += 1;
                    let _ = self
                        .add_memory_history_event(
                            target_id,
                            "SUPERSEDE",
                            &memory.text,
                            &new_id,
                            user_id,
                        )
                        .await;
                    self.emit_memory_superseded(&new_id, target_id, user_id)
                        .await;
                }
                added_ids.push(new_id.clone());
                new_id
            }
            MemoryOperation::LinkExisting | MemoryOperation::CrossContradict => {
                unreachable!("Phase 1 should not produce cross-user operations");
            }
            MemoryOperation::Add => {
                let (new_id, new_chunks) =
                    self.store_new_memory(memory, user_id, vector, tags).await?;
                *chunks_created += new_chunks;
                let _ = self
                    .add_memory_history_event(&new_id, "ADD", "", &memory.text, user_id)
                    .await;
                self.emit_memory_added(&new_id, user_id, &memory.memory_type)
                    .await;

                let exact_thr = self.config.search_thresholds.exact_duplicate_score;
                if !self.config.mode.collective_enabled() {
                    // Solo mode: never reach across users. The memory stays the
                    // writer's own — no linking, no cross-user contradictions.
                    debug!("Skipping Phase 2: solo mode (no cross-user behavior)");
                } else if phase1_similar.iter().any(|m| m.score >= exact_thr) {
                    let max_score = phase1_similar
                        .iter()
                        .map(|m| m.score)
                        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                        .unwrap_or(0.0);
                    debug!(
                        "Skipping Phase 2: exact duplicate found in Phase 1 with score {:.3}",
                        max_score
                    );
                } else {
                    self.apply_cross_user_phase(
                        memory,
                        user_id,
                        vector,
                        &new_id,
                        relations_created,
                    )
                    .await?;
                }

                added_ids.push(new_id.clone());
                new_id
            }
        };

        // P0: persist the typed edges the decision proposed (`relates_to`). This
        // is the "similar existing memory → typed edge" path; the field was
        // parsed but NEVER applied, so the graph never grew across add_memory
        // calls. Wire the resulting memory into each existing one it relates to,
        // picking the most specific edge type the model chose.
        if let Some(rels) = &decision.relates_to {
            for (target_id, edge) in rels {
                if target_id.is_empty() || target_id == &memory_id {
                    continue;
                }
                let rel_type = ReasoningType::from_token(edge);
                match self
                    .reasoning_engine
                    .add_relation(&memory_id, target_id, rel_type, 70, None)
                    .await
                {
                    Ok(_) => *relations_created += 1,
                    Err(e) => {
                        debug!("relates_to {memory_id}->{target_id} ({edge}) skipped: {e}")
                    }
                }
            }
        }

        if let Some(agent_id) = agent_id {
            let _ = self
                .link_agent_to_memory(agent_id, &memory_id, "extraction")
                .await;
        }

        Ok(Some(memory_id))
    }
}
