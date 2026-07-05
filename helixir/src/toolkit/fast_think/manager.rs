use parking_lot::RwLock;
use petgraph::stable_graph::NodeIndex;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::limits::FastThinkLimits;
use super::models::*;
use super::session::ThinkingSession;
use crate::core::HelixirClient;

pub struct FastThinkManager {
    sessions: RwLock<HashMap<String, ThinkingSession>>,
    limits: FastThinkLimits,
    main_memory: Arc<HelixirClient>,
}

impl FastThinkManager {
    pub fn new(main_memory: Arc<HelixirClient>, limits: FastThinkLimits) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            limits,
            main_memory,
        }
    }

    pub fn with_default_limits(main_memory: Arc<HelixirClient>) -> Self {
        Self::new(main_memory, FastThinkLimits::default())
    }

    pub fn start_thinking(
        &self,
        session_id: &str,
        initial_thought: &str,
    ) -> Result<NodeIndex, FastThinkError> {
        let mut sessions = self.sessions.write();

        if sessions.contains_key(session_id) {
            return Err(FastThinkError::SessionAlreadyExists);
        }

        let mut session = ThinkingSession::new(session_id);
        let node = session.add_thought(
            initial_thought,
            ThoughtType::Initial,
            None,
            None,
            &self.limits,
        )?;

        info!(
            session_id = session_id,
            thought = initial_thought,
            "Started thinking session"
        );

        sessions.insert(session_id.to_string(), session);
        Ok(node)
    }

    pub fn add_thought(
        &self,
        session_id: &str,
        content: &str,
        thought_type: ThoughtType,
        parent: Option<NodeIndex>,
        edge_type: Option<ThoughtEdge>,
    ) -> Result<NodeIndex, FastThinkError> {
        let mut sessions = self.sessions.write();
        let session = sessions
            .get_mut(session_id)
            .ok_or(FastThinkError::SessionNotFound)?;

        let node = session.add_thought(content, thought_type, parent, edge_type, &self.limits)?;

        debug!(
            session_id = session_id,
            thought_count = session.thought_count(),
            depth = session.current_depth,
            "Added thought"
        );

        Ok(node)
    }

    pub async fn recall(
        &self,
        session_id: &str,
        query: &str,
        parent_thought: NodeIndex,
        user_id: &str,
    ) -> Result<Vec<NodeIndex>, FastThinkError> {
        {
            let mut sessions = self.sessions.write();
            let session = sessions
                .get_mut(session_id)
                .ok_or(FastThinkError::SessionNotFound)?;
            session.status = SessionStatus::NeedsRecall;
            session.owner_hint = Some(user_id.to_string());
        }

        let mut memories = self
            .main_memory
            .search(
                query,
                user_id,
                Some(self.limits.max_recall_results),
                Some("contextual"),
                None,
                None,
                None,
            )
            .await
            .map_err(|e| FastThinkError::RecallFailed(e.to_string()))?;
        // #81 belt: the limit above bounds the search, but a recall must
        // never exceed max_recall_results regardless of engine behavior —
        // every recalled row becomes a session thought AND a SUPPORTS
        // provenance edge at commit, so an unclamped recall is both a
        // context-window flood for the agent and a slow commit. The score
        // floor guards the THIN-store case where the top-K itself reaches
        // into the flat expansion tail (see recall_min_score in config).
        memories.retain(|m| m.score >= self.limits.recall_min_score);
        memories.truncate(self.limits.max_recall_results);

        // #90: the belt's failure mode must not be a silent zero. A strong
        // model sharpens its query on an empty recall; a weak one concludes
        // "no evidence exists" and reasons unsupported. One fallback pass:
        // whole store (contextual is 30d — evidence for decisions is often
        // older), relaxed floor, a cap SMALLER than the primary — and every
        // fallback row is annotated as weak evidence, so the tree and the
        // SUPPORTS provenance stay honest about its quality.
        let mut weak_evidence = false;
        if memories.is_empty() && self.limits.recall_fallback_max > 0 {
            let mut wide = self
                .main_memory
                .search(
                    query,
                    user_id,
                    Some(self.limits.recall_fallback_max),
                    Some("full"),
                    None,
                    None,
                    None,
                )
                .await
                .map_err(|e| FastThinkError::RecallFailed(e.to_string()))?;
            wide.retain(|m| m.score >= self.limits.recall_fallback_min_score);
            wide.truncate(self.limits.recall_fallback_max);
            weak_evidence = !wide.is_empty();
            memories = wide;
        }

        info!(
            session_id = session_id,
            query = query,
            results = memories.len(),
            weak_evidence = weak_evidence,
            "Recalled from main memory"
        );

        let mut recalled_nodes = Vec::new();

        {
            let mut sessions = self.sessions.write();
            let session = sessions
                .get_mut(session_id)
                .ok_or(FastThinkError::SessionNotFound)?;

            for memory in memories {
                if session.thought_count() >= self.limits.max_thoughts {
                    warn!(session_id = session_id, "Hit thought limit during recall");
                    break;
                }

                let content = if weak_evidence {
                    format!(
                        "[weak recall, score {:.2} — below the primary evidence bar] {}",
                        memory.score, memory.content
                    )
                } else {
                    memory.content.clone()
                };
                let node = session.add_recalled_thought(
                    &content,
                    &memory.id,
                    memory.score,
                    parent_thought,
                    &self.limits,
                )?;

                recalled_nodes.push(node);
            }

            session.status = SessionStatus::Thinking;
        }

        Ok(recalled_nodes)
    }

    pub fn conclude(
        &self,
        session_id: &str,
        conclusion: &str,
        supporting_thoughts: &[NodeIndex],
    ) -> Result<NodeIndex, FastThinkError> {
        let mut sessions = self.sessions.write();
        let session = sessions
            .get_mut(session_id)
            .ok_or(FastThinkError::SessionNotFound)?;

        let node = session.add_conclusion(conclusion, supporting_thoughts, &self.limits)?;

        info!(
            session_id = session_id,
            supporting_count = supporting_thoughts.len(),
            "Reached conclusion"
        );

        Ok(node)
    }

    pub async fn commit(
        &self,
        session_id: &str,
        user_id: &str,
    ) -> Result<CommitResult, FastThinkError> {
        let session = {
            let mut sessions = self.sessions.write();
            sessions
                .remove(session_id)
                .ok_or(FastThinkError::SessionNotFound)?
        };

        let conclusions = session.get_conclusions();
        if conclusions.is_empty() {
            return Err(FastThinkError::NoConclusion);
        }

        let conclusion_content = session.build_conclusion_content();
        // Evidence = recalls the conclusion rests on; fall back to all recalls
        // only when the session graph is too flat to tell (old behaviour).
        let mut supporting_ids: Vec<String> = session.get_conclusion_evidence_ids();
        if supporting_ids.is_empty() {
            supporting_ids = session.get_supporting_memory_ids();
        }
        let ft = &self.main_memory.tooling().config.fast_think;

        // The session already IS the structure — conclusions are explicit,
        // typed and atomized by the agent. Re-running LLM extraction over them
        // re-discovers what we hold (and used to dominate commit latency at
        // 40-96s). Fast path: hand the conclusions to the pipeline as prepared
        // atoms — dedup, charter and typed-edge enrichment all still apply.
        // A wall-of-text conclusion still earns full extraction: atomizing it
        // is worth the wait.
        let fast = conclusion_content.len() <= ft.commit_extract_over_chars;
        let (certainty, importance, support_strength) = (
            ft.commit_certainty as i32,
            ft.commit_importance as i32,
            ft.commit_support_strength as i32,
        );

        let result = if fast {
            let atoms: Vec<crate::llm::extractor::ExtractedMemory> = conclusions
                .iter()
                .map(|(_, t)| crate::llm::extractor::ExtractedMemory {
                    text: t.content.clone(),
                    // A committed conclusion is decided-but-derived knowledge.
                    memory_type: "fact".to_string(),
                    certainty,
                    importance,
                    entities: vec![],
                    context: None,
                })
                .collect();
            self.main_memory
                .add_prepared(atoms, user_id, None, None)
                .await
        } else {
            self.main_memory
                .add(&conclusion_content, user_id, None, None)
                .await
        }
        .map_err(|e| FastThinkError::CommitFailed(e.to_string()))?;

        // Recalled evidence becomes SUPPORTS provenance edges (LLM-free) —
        // not "[Evidence: ...]" text glued into the content.
        let committed_ids: Vec<String> = if result.memory_ids.is_empty() {
            result.deduped.clone()
        } else {
            result.memory_ids.clone()
        };
        for sid in &supporting_ids {
            for mid in &committed_ids {
                if sid == mid {
                    continue;
                }
                if let Err(e) = self
                    .main_memory
                    .tooling()
                    .reasoning_engine
                    .add_relation(
                        sid,
                        mid,
                        crate::toolkit::mind_toolbox::reasoning::ReasoningType::Supports,
                        support_strength,
                        None,
                    )
                    .await
                {
                    debug!("commit: evidence SUPPORTS {sid} -> {mid} failed (non-fatal): {e}");
                }
            }
        }

        // The fast path skipped extraction, so entity discovery moves OFF the
        // critical path: one background extraction call links entities to the
        // stored conclusion after the agent already has its ack.
        if fast && !committed_ids.is_empty() {
            let client = Arc::clone(&self.main_memory);
            let text = conclusion_content.clone();
            let uid = user_id.to_string();
            let ids = committed_ids.clone();
            tokio::spawn(async move {
                client
                    .tooling()
                    .extract_and_link_entities(&text, &uid, &ids)
                    .await;
            });
        }

        let pipeline_entities = result.entities_extracted;
        let pipeline_relations = result.relations_created + supporting_ids.len();

        info!(
            session_id = session_id,
            memory_id = ?committed_ids.first(),
            fast_path = fast,
            thoughts_processed = session.thought_count(),
            entities_extracted = pipeline_entities + session.entity_count(),
            relations_created = pipeline_relations,
            elapsed_ms = session.elapsed().as_millis(),
            "Committed thinking session to main memory"
        );

        Ok(CommitResult {
            memory_id: committed_ids.first().cloned().unwrap_or_default(),
            thoughts_processed: session.thought_count(),
            entities_extracted: pipeline_entities + session.entity_count(),
            concepts_mapped: pipeline_relations + session.concept_count(),
            elapsed: session.elapsed(),
        })
    }

    pub fn discard(&self, session_id: &str) -> Result<DiscardResult, FastThinkError> {
        let mut sessions = self.sessions.write();
        let session = sessions
            .remove(session_id)
            .ok_or(FastThinkError::SessionNotFound)?;

        info!(
            session_id = session_id,
            thoughts = session.thought_count(),
            elapsed_ms = session.elapsed().as_millis(),
            "Discarded thinking session"
        );

        Ok(DiscardResult {
            thoughts_discarded: session.thought_count(),
            elapsed: session.elapsed(),
        })
    }

    pub async fn commit_partial(
        &self,
        session_id: &str,
        user_id: &str,
        reason: &str,
    ) -> Result<CommitResult, FastThinkError> {
        let session = {
            let mut sessions = self.sessions.write();
            sessions
                .remove(session_id)
                .ok_or(FastThinkError::SessionNotFound)?
        };

        let thoughts: Vec<String> = session
            .graph
            .node_indices()
            .filter_map(|idx| session.graph.node_weight(idx))
            .map(|t| format!("- [{}] {}", t.thought_type, t.content))
            .collect();

        if thoughts.is_empty() {
            return Err(FastThinkError::NoConclusion);
        }

        let partial_content = format!(
            "FastThink session interrupted ({})\n\nThoughts:\n{}\n\n[Action: Continue research with think_start]",
            reason,
            thoughts.join("\n")
        );

        // Use add_with_tags to mark as incomplete_thought - tag is inherited by all extracted facts
        let result = self
            .main_memory
            .add_with_tags(
                &partial_content,
                user_id,
                None,
                None,
                Some("incomplete_thought"),
            )
            .await
            .map_err(|e| FastThinkError::CommitFailed(e.to_string()))?;

        let pipeline_entities = result.entities_extracted;
        let pipeline_relations = result.relations_created;

        warn!(
            session_id = session_id,
            reason = reason,
            memory_id = ?result.memory_ids.first(),
            thoughts_processed = session.thought_count(),
            entities_extracted = pipeline_entities,
            "Committed PARTIAL thinking session to main memory"
        );

        Ok(CommitResult {
            memory_id: result.memory_ids.first().cloned().unwrap_or_default(),
            thoughts_processed: session.thought_count(),
            entities_extracted: pipeline_entities + session.entity_count(),
            concepts_mapped: pipeline_relations + session.concept_count(),
            elapsed: session.elapsed(),
        })
    }

    pub fn extract_entity(
        &self,
        session_id: &str,
        thought_idx: NodeIndex,
        name: &str,
        entity_type: ScratchEntityType,
    ) -> Result<String, FastThinkError> {
        let mut sessions = self.sessions.write();
        let session = sessions
            .get_mut(session_id)
            .ok_or(FastThinkError::SessionNotFound)?;

        session.extract_entity(thought_idx, name, entity_type, &self.limits)
    }

    pub fn map_to_concept(
        &self,
        session_id: &str,
        thought_idx: NodeIndex,
        concept_name: &str,
        parent_concept: Option<&str>,
    ) -> Result<String, FastThinkError> {
        let mut sessions = self.sessions.write();
        let session = sessions
            .get_mut(session_id)
            .ok_or(FastThinkError::SessionNotFound)?;

        session.map_to_concept(thought_idx, concept_name, parent_concept, &self.limits)
    }

    pub fn link_thoughts(
        &self,
        session_id: &str,
        from: NodeIndex,
        to: NodeIndex,
        edge_type: ThoughtEdge,
    ) -> Result<(), FastThinkError> {
        let mut sessions = self.sessions.write();
        let session = sessions
            .get_mut(session_id)
            .ok_or(FastThinkError::SessionNotFound)?;

        session.link_thoughts(from, to, edge_type)?;
        Ok(())
    }

    /// The session thought ceiling — exposed so surfaces (think_status) can
    /// report headroom. think_conclude bypasses this cap by design.
    pub fn max_thoughts(&self) -> usize {
        self.limits.max_thoughts
    }

    pub fn get_session_status(&self, session_id: &str) -> Result<SessionInfo, FastThinkError> {
        let sessions = self.sessions.read();
        let session = sessions
            .get(session_id)
            .ok_or(FastThinkError::SessionNotFound)?;

        Ok(SessionInfo {
            id: session.id.clone(),
            status: session.status.clone(),
            thought_count: session.thought_count(),
            entity_count: session.entity_count(),
            concept_count: session.concept_count(),
            current_depth: session.current_depth,
            elapsed: session.elapsed(),
            has_conclusion: !session.get_conclusions().is_empty(),
        })
    }

    pub fn get_thought_chain(
        &self,
        session_id: &str,
        thought_idx: NodeIndex,
    ) -> Result<Vec<ThoughtInfo>, FastThinkError> {
        let sessions = self.sessions.read();
        let session = sessions
            .get(session_id)
            .ok_or(FastThinkError::SessionNotFound)?;

        let chain = session.get_chain_to_root(thought_idx);

        Ok(chain
            .iter()
            .filter_map(|&idx| {
                session.get_thought(idx).map(|t| ThoughtInfo {
                    id: t.id.clone(),
                    content: t.content.clone(),
                    thought_type: t.thought_type.clone(),
                    certainty: t.certainty,
                    depth: t.depth,
                })
            })
            .collect())
    }

    pub fn cleanup_stale(&self) -> usize {
        let mut sessions = self.sessions.write();
        let ttl = self.limits.session_ttl;
        let before = sessions.len();

        sessions.retain(|id, session| {
            let keep = session.last_activity.elapsed() < ttl;
            if !keep {
                info!(session_id = id, "Cleaned up stale session");
            }
            keep
        });

        before - sessions.len()
    }

    pub fn active_session_count(&self) -> usize {
        self.sessions.read().len()
    }

    pub fn list_sessions(&self) -> Vec<String> {
        self.sessions.read().keys().cloned().collect()
    }

    /// Shutdown auto-save: persist every still-active session as an
    /// [INCOMPLETE] memory (via `commit_partial`) so reasoning survives the
    /// process — one-shot MCP clients kill the server long before the
    /// session-TTL sweeper would fire. Sessions with no recall never learned
    /// an owner; they save under the `helixir` system user, and
    /// `search_incomplete_thoughts` finds them regardless (tag search is
    /// user-agnostic). Returns how many sessions were saved.
    pub async fn save_all_interrupted(&self, reason: &str) -> usize {
        let ids = self.list_sessions();
        let mut saved = 0usize;
        for id in ids {
            let owner = {
                let sessions = self.sessions.read();
                sessions.get(&id).and_then(|s| s.owner_hint.clone())
            }
            .unwrap_or_else(|| "helixir".to_string());
            match self.commit_partial(&id, &owner, reason).await {
                Ok(_) => {
                    info!(session_id = %id, owner = %owner, "Auto-saved interrupted FastThink session");
                    saved += 1;
                }
                // NoConclusion = empty session — nothing worth keeping.
                Err(FastThinkError::NoConclusion) => {}
                Err(e) => warn!(session_id = %id, "Shutdown auto-save failed: {e}"),
            }
        }
        saved
    }
}

#[derive(Debug, Clone)]
pub struct CommitResult {
    pub memory_id: String,
    pub thoughts_processed: usize,
    pub entities_extracted: usize,
    pub concepts_mapped: usize,
    pub elapsed: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct DiscardResult {
    pub thoughts_discarded: usize,
    pub elapsed: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub status: SessionStatus,
    pub thought_count: usize,
    pub entity_count: usize,
    pub concept_count: usize,
    pub current_depth: usize,
    pub elapsed: std::time::Duration,
    pub has_conclusion: bool,
}

#[derive(Debug, Clone)]
pub struct ThoughtInfo {
    pub id: String,
    pub content: String,
    pub thought_type: ThoughtType,
    pub certainty: f32,
    pub depth: usize,
}
