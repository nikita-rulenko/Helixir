//! Memory CRUD methods on [`HelixirClient`]: `add`, `add_with_tags`,
//! `search`, `update`, `delete`.

use std::collections::HashMap;

use super::client::HelixirClient;
use super::error::HelixirClientError;
use super::types::{AddMemoryResult, SearchResult, UpdateResult};

impl HelixirClient {
    pub async fn add(
        &self,
        message: &str,
        user_id: &str,
        agent_id: Option<&str>,
        metadata: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<AddMemoryResult, HelixirClientError> {
        self.add_with_tags(message, user_id, agent_id, metadata, None)
            .await
    }

    /// Add memory with optional context tags that are inherited by all extracted facts.
    pub async fn add_with_tags(
        &self,
        message: &str,
        user_id: &str,
        agent_id: Option<&str>,
        metadata: Option<HashMap<String, serde_json::Value>>,
        context_tags: Option<&str>,
    ) -> Result<AddMemoryResult, HelixirClientError> {
        self.ensure_initialized().await?;

        let result = self
            .tooling_manager
            .add_memory(message, user_id, agent_id, metadata, context_tags)
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        Ok(AddMemoryResult {
            memories_added: result.added.len(),
            memory_ids: result.added,
            deduped: result.deduped,
            chunks_created: result.chunks_created,
            entities_extracted: result.entities_extracted,
            relations_created: result.reasoning_relations_created,
            stats: result.metadata,
            needs_clarification: result.needs_clarification,
        })
    }

    /// Store atoms the caller has ALREADY structured (FastThink commit) —
    /// the same pipeline as `add_with_tags` minus the extraction LLM call.
    /// Dedup, the charter and typed-edge enrichment all still apply.
    pub async fn add_prepared(
        &self,
        memories: Vec<crate::llm::extractor::ExtractedMemory>,
        user_id: &str,
        agent_id: Option<&str>,
        context_tags: Option<&str>,
    ) -> Result<AddMemoryResult, HelixirClientError> {
        self.ensure_initialized().await?;

        let result = self
            .tooling_manager
            .add_prepared_memories(memories, user_id, agent_id, context_tags)
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        Ok(AddMemoryResult {
            memories_added: result.added.len(),
            memory_ids: result.added,
            deduped: result.deduped,
            chunks_created: result.chunks_created,
            entities_extracted: result.entities_extracted,
            relations_created: result.reasoning_relations_created,
            stats: result.metadata,
            needs_clarification: result.needs_clarification,
        })
    }

    /// Ingest buffer (#25): persist the raw input and return a `pending_id`
    /// immediately. A background worker drains the queue serially. Use
    /// [`Self::add_status`] to poll for the result.
    pub async fn add_buffered(
        &self,
        message: &str,
        user_id: &str,
        agent_id: Option<&str>,
        context_tags: Option<&str>,
    ) -> Result<crate::toolkit::tooling_manager::ingest_buffer::EnqueuedInput, HelixirClientError>
    {
        self.ensure_initialized().await?;
        self.tooling_manager
            .enqueue_input(message, user_id, agent_id, context_tags)
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))
    }

    /// Poll a queued input's status (and result once done).
    pub async fn add_status(
        &self,
        pending_id: &str,
    ) -> Result<crate::toolkit::tooling_manager::ingest_buffer::PendingStatus, HelixirClientError>
    {
        self.ensure_initialized().await?;
        self.tooling_manager
            .pending_status(pending_id)
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))
    }

    /// Confirm-or-promise (#63): poll a queued input until it reaches a
    /// terminal state (done/failed) or the wait budget runs out. Returns the
    /// terminal [`PendingStatus`], or `None` if it is still processing when the
    /// budget ends (the caller then returns an explicit "accepted" ack).
    ///
    /// This only *waits* — the serial worker still processes the queue one item
    /// at a time, so the buffer's parallel-write dedup-race protection is
    /// preserved. We just hand the caller a trustworthy result instead of a
    /// bare "pending" it would misread as failure.
    pub async fn await_add(
        &self,
        pending_id: &str,
        max_wait_ms: u64,
        poll_ms: u64,
    ) -> Option<crate::toolkit::tooling_manager::ingest_buffer::PendingStatus> {
        use crate::toolkit::tooling_manager::ingest_buffer::{STATUS_DONE, STATUS_FAILED};
        let poll = poll_ms.max(20);
        let mut waited = 0u64;
        loop {
            if let Ok(st) = self.add_status(pending_id).await {
                if st.status == STATUS_DONE || st.status == STATUS_FAILED {
                    return Some(st);
                }
            }
            if waited >= max_wait_ms {
                return None;
            }
            let step = poll.min(max_wait_ms - waited);
            tokio::time::sleep(std::time::Duration::from_millis(step)).await;
            waited += step;
        }
    }

    /// Drain the user's outbox (прихожая): completed adds and escalations
    /// that landed while the agent was away. Marks them delivered and prunes
    /// their queue tombstones. The session-start counterpart to the buffer.
    pub async fn drain_notices(
        &self,
        user_id: &str,
        limit: usize,
    ) -> Result<Vec<crate::toolkit::tooling_manager::ingest_buffer::MemoryNotice>, HelixirClientError>
    {
        self.ensure_initialized().await?;
        self.tooling_manager
            .drain_notices(user_id, limit)
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))
    }

    pub async fn search(
        &self,
        query: &str,
        user_id: &str,
        limit: Option<usize>,
        search_mode: Option<&str>,
        temporal_days: Option<f64>,
        graph_depth: Option<usize>,
        scope: Option<&str>,
    ) -> Result<Vec<SearchResult>, HelixirClientError> {
        self.search_windowed(
            query,
            user_id,
            limit,
            search_mode,
            temporal_days,
            graph_depth,
            scope,
            crate::core::TimeWindow::default(),
        )
        .await
    }

    /// #87: search bounded by an explicit two-sided EVENT-time window.
    /// Out-of-window rows reachable through the graph come back flagged
    /// as flashbacks (`metadata.flashback` + `event_date`).
    #[allow(clippy::too_many_arguments)]
    pub async fn search_windowed(
        &self,
        query: &str,
        user_id: &str,
        limit: Option<usize>,
        search_mode: Option<&str>,
        temporal_days: Option<f64>,
        graph_depth: Option<usize>,
        scope: Option<&str>,
        window: crate::core::TimeWindow,
    ) -> Result<Vec<SearchResult>, HelixirClientError> {
        self.ensure_initialized().await?;

        let mode = search_mode.unwrap_or(&self.config.default_search_mode);
        let results = self
            .tooling_manager
            .search_memory_windowed(
                query,
                user_id,
                limit,
                mode,
                temporal_days,
                graph_depth,
                scope.unwrap_or("personal"),
                window,
            )
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        Ok(results
            .into_iter()
            .map(|r| SearchResult {
                id: r.memory_id,
                content: r.content,
                score: r.score as f32,
                metadata: r.metadata,
                created_at: r.created_at,
            })
            .collect())
    }

    pub async fn update(
        &self,
        memory_id: &str,
        new_content: &str,
        user_id: &str,
    ) -> Result<UpdateResult, HelixirClientError> {
        self.ensure_initialized().await?;

        let updated = self
            .tooling_manager
            .update_memory(memory_id, new_content, user_id)
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        Ok(UpdateResult {
            memory_id: memory_id.to_string(),
            updated,
            new_content: new_content.to_string(),
        })
    }

    pub async fn delete(&self, memory_id: &str) -> Result<bool, HelixirClientError> {
        self.ensure_initialized().await?;

        self.tooling_manager
            .delete_memory(memory_id)
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))
    }
}
