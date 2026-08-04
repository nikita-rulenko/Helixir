//! Memory read, update, and buffered-status operations.

use super::*;

impl HelixirClient {
    /// Poll a queued input's status (and result once done).
    pub(crate) async fn add_status(
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

    /// Poll a buffered write after authorizing the principal against the
    /// persisted owner/creator metadata.
    pub async fn add_status_as(
        &self,
        actor_id: &str,
        pending_id: &str,
    ) -> Result<crate::toolkit::tooling_manager::ingest_buffer::PendingStatus, HelixirClientError>
    {
        let status = self.add_status(pending_id).await?;
        if status.status != "not_found" {
            self.rbac()
                .authorize_pending_read(actor_id, &status.owner_id, &status.creator_id)
                .await
                .map_err(|e| HelixirClientError::Operation(e.to_string()))?;
        }
        Ok(status)
    }

    /// Confirm-or-promise (#63): poll a queued input until it reaches a
    /// terminal state (done/failed) or the wait budget runs out. Returns the
    /// terminal `PendingStatus`, or `None` if it is still processing when the
    /// budget ends (the caller then returns an explicit "accepted" ack).
    ///
    /// This only *waits* — the serial worker still processes the queue one item
    /// at a time, so the buffer's parallel-write dedup-race protection is
    /// preserved. We just hand the caller a trustworthy result instead of a
    /// bare "pending" it would misread as failure.
    pub(crate) async fn await_add_as(
        &self,
        actor_id: &str,
        pending_id: &str,
        max_wait_ms: u64,
        poll_ms: u64,
    ) -> Option<crate::toolkit::tooling_manager::ingest_buffer::PendingStatus> {
        use crate::toolkit::tooling_manager::ingest_buffer::{STATUS_DONE, STATUS_FAILED};
        let poll = poll_ms.max(20);
        let mut waited = 0u64;
        loop {
            if let Ok(st) = self.add_status_as(actor_id, pending_id).await
                && (st.status == STATUS_DONE || st.status == STATUS_FAILED)
            {
                return Some(st);
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
    pub async fn drain_notices_as(
        &self,
        actor_id: &str,
        user_id: &str,
        limit: usize,
    ) -> Result<Vec<crate::toolkit::tooling_manager::ingest_buffer::MemoryNotice>, HelixirClientError>
    {
        self.ensure_initialized().await?;
        self.rbac()
            .authorize_outbox_read(actor_id, user_id)
            .await
            .map_err(|e| HelixirClientError::Operation(e.to_string()))?;
        self.tooling_manager
            .drain_notices(user_id, limit)
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))
    }

    /// Search the memory. Every unset [`SearchParams`] field means "the
    /// configured default". #87: an active `window` bounds recall by EVENT
    /// time; out-of-window rows reachable through the graph come back
    /// flagged as flashbacks (`metadata.flashback` + `event_date`).
    pub async fn search(
        &self,
        query: &str,
        user_id: &str,
        params: SearchParams,
    ) -> Result<Vec<SearchResult>, HelixirClientError> {
        self.search_as(user_id, query, user_id, params).await
    }

    /// Search memories as an authenticated actor, optionally targeting another owner.
    pub async fn search_as(
        &self,
        actor_id: &str,
        query: &str,
        owner_id: &str,
        params: SearchParams,
    ) -> Result<Vec<SearchResult>, HelixirClientError> {
        self.ensure_initialized().await?;

        let mode = params
            .search_mode
            .as_deref()
            .unwrap_or(&self.config.default_search_mode);
        let results = self
            .tooling_manager
            .search_memory(
                query,
                owner_id,
                crate::toolkit::tooling_manager::MemorySearchOptions {
                    limit: params.limit,
                    mode: mode.to_string(),
                    temporal_days: params.temporal_days,
                    graph_depth: params.graph_depth,
                    scope: params.scope.unwrap_or_else(|| "personal".to_string()),
                    window: params.window,
                },
            )
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        let memory_ids = results
            .iter()
            .map(|result| result.memory_id.clone())
            .collect::<Vec<_>>();
        let visible = self
            .rbac()
            .visible_memory_ids(actor_id, &memory_ids)
            .await
            .map_err(|e| HelixirClientError::Operation(e.to_string()))?;
        let results = results
            .into_iter()
            .filter(|result| {
                visible
                    .as_ref()
                    .is_none_or(|allowed| allowed.contains(&result.memory_id))
            })
            .collect::<Vec<_>>();

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
        self.update_as(user_id, memory_id, new_content).await
    }

    /// Update a memory as an authenticated actor. The stored owner is loaded
    /// from HelixDB, so callers cannot change authorship by changing a parameter.
    pub async fn update_as(
        &self,
        actor_id: &str,
        memory_id: &str,
        new_content: &str,
    ) -> Result<UpdateResult, HelixirClientError> {
        self.ensure_initialized().await?;

        let policy = self
            .rbac()
            .snapshot()
            .await
            .map_err(|e| HelixirClientError::Operation(e.to_string()))?;
        if policy.enabled {
            let response: serde_json::Value = self
                .db
                .execute_query("getMemory", &serde_json::json!({"memory_id": memory_id}))
                .await
                .map_err(|e| HelixirClientError::Operation(format!("load memory owner: {e}")))?;
            let owner = response
                .get("memory")
                .and_then(|memory| memory.get("user_id"))
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if owner.is_empty() {
                return Err(HelixirClientError::Operation(format!(
                    "RBAC denied update for actor '{actor_id}' on memory '{memory_id}'"
                )));
            }
            self.rbac()
                .authorize_memory_write(actor_id, owner, memory_id)
                .await
                .map_err(|e| HelixirClientError::Operation(e.to_string()))?;
        }

        let updated = self
            .tooling_manager
            .update_memory(memory_id, new_content, actor_id)
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        Ok(UpdateResult {
            memory_id: memory_id.to_string(),
            updated,
            new_content: new_content.to_string(),
        })
    }

    pub async fn delete(&self, memory_id: &str) -> Result<bool, HelixirClientError> {
        // The legacy signature has no authenticated actor.  It remains usable
        // only while RBAC is disabled; enabled deployments must call
        // `delete_as` so ownership is checked before mutation.
        self.delete_as("", memory_id).await
    }

    /// Delete a memory as an authenticated actor.  This administrative API is
    /// not exposed over MCP, but it still honors the same owner policy.
    pub async fn delete_as(
        &self,
        actor_id: &str,
        memory_id: &str,
    ) -> Result<bool, HelixirClientError> {
        self.ensure_initialized().await?;

        let policy = self
            .rbac()
            .snapshot()
            .await
            .map_err(|e| HelixirClientError::Operation(e.to_string()))?;
        if policy.enabled {
            let response: serde_json::Value = self
                .db
                .execute_query("getMemory", &serde_json::json!({"memory_id": memory_id}))
                .await
                .map_err(|e| HelixirClientError::Operation(format!("load memory owner: {e}")))?;
            let owner = response
                .get("memory")
                .and_then(|memory| memory.get("user_id"))
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if owner.is_empty() {
                return Err(HelixirClientError::Operation(format!(
                    "RBAC denied delete for actor '{actor_id}' on memory '{memory_id}'"
                )));
            }
            self.rbac()
                .authorize_memory_write(actor_id, owner, memory_id)
                .await
                .map_err(|e| HelixirClientError::Operation(e.to_string()))?;
        }

        self.tooling_manager
            .delete_memory(memory_id)
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))
    }
}
