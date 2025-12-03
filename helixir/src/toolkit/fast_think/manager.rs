use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use petgraph::stable_graph::NodeIndex;
use tracing::{debug, info, warn};

use crate::core::HelixirClient;
use super::models::*;
use super::session::ThinkingSession;
use super::limits::FastThinkLimits;

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

    pub fn start_thinking(&self, session_id: &str, initial_thought: &str) -> Result<NodeIndex, FastThinkError> {
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
        let session = sessions.get_mut(session_id).ok_or(FastThinkError::SessionNotFound)?;

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
    ) -> Result<Vec<NodeIndex>, FastThinkError> {
        {
            let mut sessions = self.sessions.write();
            let session = sessions.get_mut(session_id).ok_or(FastThinkError::SessionNotFound)?;
            session.status = SessionStatus::NeedsRecall;
        }

        let memories = self
            .main_memory
            .search(
                query,
                "contextual",
                Some(self.limits.max_recall_results),
                None,
                None,
                None,
            )
            .await
            .map_err(|e| FastThinkError::RecallFailed(e.to_string()))?;

        info!(
            session_id = session_id,
            query = query,
            results = memories.len(),
            "Recalled from main memory"
        );

        let mut recalled_nodes = Vec::new();

        {
            let mut sessions = self.sessions.write();
            let session = sessions.get_mut(session_id).ok_or(FastThinkError::SessionNotFound)?;

            for memory in memories {
                if session.thought_count() >= self.limits.max_thoughts {
                    warn!(
                        session_id = session_id,
                        "Hit thought limit during recall"
                    );
                    break;
                }

                let node = session.add_recalled_thought(
                    &memory.content,
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
        let session = sessions.get_mut(session_id).ok_or(FastThinkError::SessionNotFound)?;

        let node = session.add_conclusion(conclusion, supporting_thoughts, &self.limits)?;

        info!(
            session_id = session_id,
            supporting_count = supporting_thoughts.len(),
            "Reached conclusion"
        );

        Ok(node)
    }

    pub async fn commit(&self, session_id: &str, user_id: &str) -> Result<CommitResult, FastThinkError> {
        let session = {
            let mut sessions = self.sessions.write();
            sessions.remove(session_id).ok_or(FastThinkError::SessionNotFound)?
        };

        if session.get_conclusions().is_empty() {
            return Err(FastThinkError::NoConclusion);
        }

        let conclusion_content = session.build_conclusion_content();
        let supporting_evidence = session.get_supporting_evidence();

        let full_content = if supporting_evidence.is_empty() {
            conclusion_content
        } else {
            format!(
                "{}\n\n[Based on: {}]",
                conclusion_content,
                supporting_evidence.join("; ")
            )
        };

        let result = self
            .main_memory
            .add(&full_content, user_id, None, None)
            .await
            .map_err(|e| FastThinkError::CommitFailed(e.to_string()))?;

        info!(
            session_id = session_id,
            memory_id = ?result.memory_ids.first(),
            thoughts_processed = session.thought_count(),
            entities_extracted = session.entity_count(),
            elapsed_ms = session.elapsed().as_millis(),
            "Committed thinking session to main memory"
        );

        Ok(CommitResult {
            memory_id: result.memory_ids.first().cloned().unwrap_or_default(),
            thoughts_processed: session.thought_count(),
            entities_extracted: session.entity_count(),
            concepts_mapped: session.concept_count(),
            elapsed: session.elapsed(),
        })
    }

    pub fn discard(&self, session_id: &str) -> Result<DiscardResult, FastThinkError> {
        let mut sessions = self.sessions.write();
        let session = sessions.remove(session_id).ok_or(FastThinkError::SessionNotFound)?;

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

    pub async fn commit_partial(&self, session_id: &str, user_id: &str, reason: &str) -> Result<CommitResult, FastThinkError> {
        let session = {
            let mut sessions = self.sessions.write();
            sessions.remove(session_id).ok_or(FastThinkError::SessionNotFound)?
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
            .add_with_tags(&partial_content, user_id, None, None, Some("incomplete_thought"))
            .await
            .map_err(|e| FastThinkError::CommitFailed(e.to_string()))?;

        warn!(
            session_id = session_id,
            reason = reason,
            memory_id = ?result.memory_ids.first(),
            thoughts_processed = session.thought_count(),
            "Committed PARTIAL thinking session to main memory"
        );

        Ok(CommitResult {
            memory_id: result.memory_ids.first().cloned().unwrap_or_default(),
            thoughts_processed: session.thought_count(),
            entities_extracted: session.entity_count(),
            concepts_mapped: session.concept_count(),
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
        let session = sessions.get_mut(session_id).ok_or(FastThinkError::SessionNotFound)?;

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
        let session = sessions.get_mut(session_id).ok_or(FastThinkError::SessionNotFound)?;

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
        let session = sessions.get_mut(session_id).ok_or(FastThinkError::SessionNotFound)?;

        session.link_thoughts(from, to, edge_type)?;
        Ok(())
    }

    pub fn get_session_status(&self, session_id: &str) -> Result<SessionInfo, FastThinkError> {
        let sessions = self.sessions.read();
        let session = sessions.get(session_id).ok_or(FastThinkError::SessionNotFound)?;

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
        let session = sessions.get(session_id).ok_or(FastThinkError::SessionNotFound)?;

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

