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
        self.ensure_initialized().await?;

        let mode = search_mode.unwrap_or(&self.config.default_search_mode);
        let results = self
            .tooling_manager
            .search_memory(
                query,
                user_id,
                limit,
                mode,
                temporal_days,
                graph_depth,
                scope.unwrap_or("personal"),
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
