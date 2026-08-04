//! Memory write operations exposed by the client facade.

use super::*;

impl HelixirClient {
    pub async fn add(
        &self,
        message: &str,
        user_id: &str,
        agent_id: Option<&str>,
        metadata: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<AddMemoryResult, HelixirClientError> {
        self.add_as(user_id, message, user_id, agent_id, metadata)
            .await
    }

    /// Add a memory on behalf of an owner while authorizing the authenticated actor.
    /// The owner remains the memory's provenance identity; `actor_id` is never
    /// substituted into the stored `user_id` field.
    pub async fn add_as(
        &self,
        actor_id: &str,
        message: &str,
        owner_id: &str,
        agent_id: Option<&str>,
        metadata: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<AddMemoryResult, HelixirClientError> {
        self.add_as_in_group(actor_id, message, owner_id, agent_id, metadata, None)
            .await
    }

    /// Add a memory with an explicit RBAC group. Enabled non-admin callers
    /// must provide `group_id`; disabled deployments retain legacy behavior.
    pub async fn add_as_in_group(
        &self,
        actor_id: &str,
        message: &str,
        owner_id: &str,
        agent_id: Option<&str>,
        metadata: Option<HashMap<String, serde_json::Value>>,
        group_id: Option<&str>,
    ) -> Result<AddMemoryResult, HelixirClientError> {
        self.add_with_tags_as_in_group(
            actor_id, message, owner_id, agent_id, metadata, None, group_id,
        )
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
        self.add_with_tags_as(user_id, message, user_id, agent_id, metadata, context_tags)
            .await
    }

    /// Add a tagged memory with a distinct authenticated actor and owner.
    pub async fn add_with_tags_as(
        &self,
        actor_id: &str,
        message: &str,
        owner_id: &str,
        agent_id: Option<&str>,
        metadata: Option<HashMap<String, serde_json::Value>>,
        context_tags: Option<&str>,
    ) -> Result<AddMemoryResult, HelixirClientError> {
        self.add_with_tags_as_in_group(
            actor_id,
            message,
            owner_id,
            agent_id,
            metadata,
            context_tags,
            None,
        )
        .await
    }

    /// Add a tagged memory to one explicit RBAC group.
    #[allow(clippy::too_many_arguments)] // Public boundary keeps actor, owner, and group explicit.
    pub async fn add_with_tags_as_in_group(
        &self,
        actor_id: &str,
        message: &str,
        owner_id: &str,
        agent_id: Option<&str>,
        metadata: Option<HashMap<String, serde_json::Value>>,
        context_tags: Option<&str>,
        group_id: Option<&str>,
    ) -> Result<AddMemoryResult, HelixirClientError> {
        self.ensure_initialized().await?;

        let rbac = self.rbac();
        let authorized = rbac
            .authorize_and_resolve_write_scope(actor_id, owner_id, group_id)
            .await
            .map_err(|e| HelixirClientError::Operation(e.to_string()))?;

        let result = self
            .tooling_manager
            .add_memory_scoped(
                message,
                owner_id,
                agent_id,
                metadata,
                context_tags,
                &authorized.scope,
                actor_id,
            )
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        Ok(result.into())
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
        self.add_prepared_as(user_id, memories, user_id, agent_id, context_tags)
            .await
    }

    /// Store prepared memories with a distinct authenticated actor and owner.
    pub async fn add_prepared_as(
        &self,
        actor_id: &str,
        memories: Vec<crate::llm::extractor::ExtractedMemory>,
        owner_id: &str,
        agent_id: Option<&str>,
        context_tags: Option<&str>,
    ) -> Result<AddMemoryResult, HelixirClientError> {
        self.add_prepared_as_in_group(actor_id, memories, owner_id, agent_id, context_tags, None)
            .await
    }

    /// Store prepared memories in one explicit RBAC group.
    pub async fn add_prepared_as_in_group(
        &self,
        actor_id: &str,
        memories: Vec<crate::llm::extractor::ExtractedMemory>,
        owner_id: &str,
        agent_id: Option<&str>,
        context_tags: Option<&str>,
        group_id: Option<&str>,
    ) -> Result<AddMemoryResult, HelixirClientError> {
        self.ensure_initialized().await?;

        let rbac = self.rbac();
        let authorized = rbac
            .authorize_and_resolve_write_scope(actor_id, owner_id, group_id)
            .await
            .map_err(|e| HelixirClientError::Operation(e.to_string()))?;

        let result = self
            .tooling_manager
            .add_prepared_memories_scoped(
                memories,
                owner_id,
                agent_id,
                context_tags,
                &authorized.scope,
                actor_id,
            )
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        Ok(result.into())
    }

    /// Ingest buffer (#25): persist the raw input and return a `pending_id`
    /// immediately. A background worker drains the queue serially. Use
    /// [`Self::add_status_as`] to poll for the result.
    pub async fn add_buffered(
        &self,
        message: &str,
        user_id: &str,
        agent_id: Option<&str>,
        context_tags: Option<&str>,
    ) -> Result<crate::toolkit::tooling_manager::ingest_buffer::EnqueuedInput, HelixirClientError>
    {
        self.add_buffered_as(user_id, message, user_id, agent_id, context_tags)
            .await
    }

    /// Queue a memory with a distinct authenticated actor and owner.
    pub async fn add_buffered_as(
        &self,
        actor_id: &str,
        message: &str,
        owner_id: &str,
        agent_id: Option<&str>,
        context_tags: Option<&str>,
    ) -> Result<crate::toolkit::tooling_manager::ingest_buffer::EnqueuedInput, HelixirClientError>
    {
        self.add_buffered_as_in_group(actor_id, message, owner_id, agent_id, context_tags, None)
            .await
    }

    /// Queue a memory for one explicit RBAC group.
    pub async fn add_buffered_as_in_group(
        &self,
        actor_id: &str,
        message: &str,
        owner_id: &str,
        agent_id: Option<&str>,
        context_tags: Option<&str>,
        group_id: Option<&str>,
    ) -> Result<crate::toolkit::tooling_manager::ingest_buffer::EnqueuedInput, HelixirClientError>
    {
        self.ensure_initialized().await?;
        let authorized = self
            .rbac()
            .authorize_and_resolve_write_scope(actor_id, owner_id, group_id)
            .await
            .map_err(|e| HelixirClientError::Operation(e.to_string()))?;
        self.tooling_manager
            .enqueue_input(
                message,
                owner_id,
                actor_id,
                agent_id,
                context_tags,
                authorized.group_id.as_deref(),
            )
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))
    }
}
