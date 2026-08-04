//! FastThink session lifecycle and scratchpad operations.

use super::*;

impl FastThinkManager {
    pub(crate) fn new(main_memory: Arc<HelixirClient>, limits: FastThinkLimits) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            current: arc_swap::ArcSwap::from_pointee(FastThinkRuntime {
                limits,
                main_memory,
            }),
        }
    }

    /// Publish a new runtime generation for sessions started after a hot
    /// reload. Existing sessions retain the client and limits they began
    /// with, so their reasoning remains internally consistent.
    pub fn update_runtime(&self, main_memory: Arc<HelixirClient>, limits: FastThinkLimits) {
        self.current.store(Arc::new(FastThinkRuntime {
            limits,
            main_memory,
        }));
    }

    pub fn start_thinking(
        &self,
        session_id: &str,
        initial_thought: &str,
        actor_id: Option<&str>,
    ) -> Result<NodeIndex, FastThinkError> {
        let mut sessions = self.sessions.write();

        if sessions.contains_key(session_id) {
            return Err(FastThinkError::SessionAlreadyExists);
        }

        let runtime = self.current.load_full();
        let mut session = ThinkingSession::new(session_id, actor_id);
        let node = session.add_thought(
            initial_thought,
            ThoughtType::Initial,
            None,
            None,
            &runtime.limits,
        )?;

        info!(
            session_id = session_id,
            thought = initial_thought,
            "Started thinking session"
        );

        sessions.insert(
            session_id.to_string(),
            ManagedSession {
                state: session,
                runtime,
            },
        );
        Ok(node)
    }

    pub fn add_thought(
        &self,
        session_id: &str,
        actor_id: Option<&str>,
        content: &str,
        thought_type: ThoughtType,
        parent: Option<NodeIndex>,
        edge_type: Option<ThoughtEdge>,
    ) -> Result<NodeIndex, FastThinkError> {
        let mut sessions = self.sessions.write();
        let session = sessions
            .get_mut(session_id)
            .ok_or(FastThinkError::SessionNotFound)?;
        session.authorize_actor(actor_id)?;

        let runtime = Arc::clone(&session.runtime);
        let node =
            session.add_thought(content, thought_type, parent, edge_type, &runtime.limits)?;

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
        self.recall_as(session_id, query, parent_thought, None, user_id, user_id)
            .await
    }

    pub async fn recall_as(
        &self,
        session_id: &str,
        query: &str,
        parent_thought: NodeIndex,
        session_actor: Option<&str>,
        actor_id: &str,
        user_id: &str,
    ) -> Result<Vec<NodeIndex>, FastThinkError> {
        let runtime = {
            let mut sessions = self.sessions.write();
            let session = sessions
                .get_mut(session_id)
                .ok_or(FastThinkError::SessionNotFound)?;
            session.authorize_actor(session_actor)?;
            session.status = SessionStatus::NeedsRecall;
            session.owner_hint = Some(user_id.to_string());
            Arc::clone(&session.runtime)
        };

        let mut memories = runtime
            .main_memory
            .search_as(
                actor_id,
                query,
                user_id,
                crate::core::helixir_client::SearchParams {
                    limit: Some(runtime.limits.max_recall_results),
                    search_mode: Some("contextual".to_string()),
                    ..Default::default()
                },
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
        memories.retain(|m| m.score >= runtime.limits.recall_min_score);
        memories.truncate(runtime.limits.max_recall_results);

        // #90: the belt's failure mode must not be a silent zero. A strong
        // model sharpens its query on an empty recall; a weak one concludes
        // "no evidence exists" and reasons unsupported. One fallback pass:
        // whole store (contextual is 30d — evidence for decisions is often
        // older), relaxed floor, a cap SMALLER than the primary — and every
        // fallback row is annotated as weak evidence, so the tree and the
        // SUPPORTS provenance stay honest about its quality.
        let mut weak_evidence = false;
        if memories.is_empty() && runtime.limits.recall_fallback_max > 0 {
            let mut wide = runtime
                .main_memory
                .search_as(
                    actor_id,
                    query,
                    user_id,
                    crate::core::helixir_client::SearchParams {
                        limit: Some(runtime.limits.recall_fallback_max),
                        search_mode: Some("full".to_string()),
                        ..Default::default()
                    },
                )
                .await
                .map_err(|e| FastThinkError::RecallFailed(e.to_string()))?;
            wide.retain(|m| m.score >= runtime.limits.recall_fallback_min_score);
            wide.truncate(runtime.limits.recall_fallback_max);
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
            session.authorize_actor(session_actor)?;

            // #78: recalled evidence must never trap the session at the cap —
            // stop short so a synthesis thought + the conclusion always fit.
            let recall_ceiling = runtime
                .limits
                .max_thoughts
                .saturating_sub(runtime.limits.conclude_reserve);
            for memory in memories {
                if session.thought_count() >= recall_ceiling {
                    warn!(
                        session_id = session_id,
                        "Recall stopped at the reserve ceiling ({recall_ceiling} of {} thoughts) — headroom kept for synthesis + conclude",
                        runtime.limits.max_thoughts
                    );
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
                    &runtime.limits,
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
        actor_id: Option<&str>,
        conclusion: &str,
        supporting_thoughts: &[NodeIndex],
    ) -> Result<NodeIndex, FastThinkError> {
        let mut sessions = self.sessions.write();
        let session = sessions
            .get_mut(session_id)
            .ok_or(FastThinkError::SessionNotFound)?;
        session.authorize_actor(actor_id)?;

        let runtime = Arc::clone(&session.runtime);
        let node = session.add_conclusion(conclusion, supporting_thoughts, &runtime.limits)?;

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
        self.commit_as_in_group(session_id, None, user_id, user_id, None)
            .await
    }

    pub async fn commit_as(
        &self,
        session_id: &str,
        actor_id: &str,
        user_id: &str,
    ) -> Result<CommitResult, FastThinkError> {
        self.commit_as_in_group(session_id, None, actor_id, user_id, None)
            .await
    }
}
