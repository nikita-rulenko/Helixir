//! Persistent ingest buffer ("предбанник", #25).
//!
//! When `HELIXIR_INGEST_BUFFER=1`, `add_memory` does not run the full
//! (LLM-heavy) pipeline inline. Instead it persists the raw input as a
//! `PendingInput` node in HelixDB and returns a `pending_id` instantly. A
//! single background worker drains the queue **serially** through the normal
//! `add_memory` pipeline and records the result back on the node.
//!
//! Two properties this buys, both load-bearing:
//! - **Latency hiding**: a 14B-class local extractor (~17 s) is acceptable
//!   when it grinds in the background instead of blocking the caller.
//! - **Dedup-race closure**: parallel writers used to read the same DB
//!   snapshot and both decide ADD. One serial worker sees each prior write
//!   before the next, so the race cannot occur by construction.
//!
//! Durability is the same as memory itself — the queue is HelixDB nodes, so
//! an ack survives process death. The synchronous path is untouched and
//! remains the default (backward compatible); the buffer is strictly opt-in.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::ToolingManager;
use super::types::ToolingError;

/// A queued input's lifecycle. Stored as the `status` string on the node.
pub const STATUS_PENDING: &str = "pending";
pub const STATUS_PROCESSING: &str = "processing";
pub const STATUS_DONE: &str = "done";
pub const STATUS_FAILED: &str = "failed";

/// Server-side auto-retry budget for a queued write (#25). The agent never
/// sees these — write-failure handling is entirely internal.
const INGEST_MAX_RETRIES: u32 = 5;
const INGEST_DEADLINE: Duration = Duration::from_secs(60);

/// Returned to the agent when the buffer accepts an input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnqueuedInput {
    pub pending_id: String,
    pub status: String,
    pub queued: bool,
}

/// Status of a queued input, polled by the agent via `get_memory_status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingStatus {
    pub pending_id: String,
    pub status: String,
    /// Present when `status == done`: the JSON the synchronous path would
    /// have returned (memory_ids, needs_clarification, counts).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PendingNode {
    pending_id: String,
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    raw_message: String,
    #[serde(default)]
    agent_id: String,
    #[serde(default)]
    context_tags: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    result: String,
    #[serde(default)]
    error: String,
}

#[derive(Debug, Deserialize)]
struct PendingOne {
    #[serde(default)]
    pending: Option<PendingNode>,
}

#[derive(Debug, Deserialize)]
struct PendingList {
    #[serde(default)]
    pending: Vec<PendingNode>,
}

/// One outbox item, returned to the agent on drain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryNotice {
    pub notice_id: String,
    pub kind: String,
    pub payload: serde_json::Value,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub pending_id: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
struct NoticeNode {
    notice_id: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    payload: String,
    #[serde(default)]
    pending_id: String,
    #[serde(default)]
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct NoticeList {
    #[serde(default)]
    notices: Vec<NoticeNode>,
}

/// Is the ingest buffer active for this process?
pub fn buffer_enabled() -> bool {
    std::env::var("HELIXIR_INGEST_BUFFER")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn poll_interval() -> Duration {
    let ms = std::env::var("HELIXIR_INGEST_POLL_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(500);
    Duration::from_millis(ms.clamp(50, 60_000))
}

impl ToolingManager {
    /// Persist a raw input to the queue and return its id immediately.
    pub async fn enqueue_input(
        &self,
        message: &str,
        user_id: &str,
        agent_id: Option<&str>,
        context_tags: Option<&str>,
    ) -> Result<EnqueuedInput, ToolingError> {
        let pending_id = format!("pi_{}", Uuid::new_v4().simple());
        let params = serde_json::json!({
            "pending_id": pending_id,
            "user_id": user_id,
            "raw_message": message,
            "agent_id": agent_id.unwrap_or(""),
            "context_tags": context_tags.unwrap_or(""),
            "status": STATUS_PENDING,
            "created_at": chrono::Utc::now().to_rfc3339(),
        });
        self.db
            .execute_query::<serde_json::Value, _>("enqueuePendingInput", &params)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        info!("Ingest buffer: queued {pending_id} for user {user_id}");
        Ok(EnqueuedInput {
            pending_id,
            status: STATUS_PENDING.to_string(),
            queued: true,
        })
    }

    /// Poll a queued input's status (and result once done).
    pub async fn pending_status(&self, pending_id: &str) -> Result<PendingStatus, ToolingError> {
        let params = serde_json::json!({ "pending_id": pending_id });
        // A pruned (delivered) or never-existing tombstone makes the
        // `::FIRST` traversal raise GRAPH_ERROR "No value found" (same shape
        // as issue #19). Both mean "not in the queue" — map to not_found
        // instead of bubbling a hard error.
        let resp: PendingOne = match self.db.execute_query("getPendingInput", &params).await {
            Ok(r) => r,
            Err(e) if e.to_string().to_lowercase().contains("no value found") => {
                return Ok(PendingStatus {
                    pending_id: pending_id.to_string(),
                    status: "not_found".to_string(),
                    result: None,
                    error: None,
                });
            }
            Err(e) => return Err(ToolingError::Database(e.to_string())),
        };

        let Some(node) = resp.pending else {
            return Ok(PendingStatus {
                pending_id: pending_id.to_string(),
                status: "not_found".to_string(),
                result: None,
                error: None,
            });
        };

        let result = (!node.result.is_empty())
            .then(|| serde_json::from_str(&node.result).ok())
            .flatten();
        let error = (!node.error.is_empty()).then(|| node.error.clone());

        Ok(PendingStatus {
            pending_id: node.pending_id,
            status: node.status,
            result,
            error,
        })
    }

    async fn fetch_pending_batch(&self, limit: i64) -> Result<Vec<PendingNode>, ToolingError> {
        let params = serde_json::json!({ "status": STATUS_PENDING, "limit": limit });
        let resp: PendingList = self
            .db
            .execute_query("getPendingInputsByStatus", &params)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        Ok(resp.pending)
    }

    async fn set_pending_status(
        &self,
        pending_id: &str,
        status: &str,
        result: &str,
        error: &str,
    ) -> Result<(), ToolingError> {
        let params = serde_json::json!({
            "pending_id": pending_id,
            "status": status,
            "processed_at": chrono::Utc::now().to_rfc3339(),
            "result": result,
            "error": error,
        });
        self.db
            .execute_query::<serde_json::Value, _>("updatePendingInput", &params)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        Ok(())
    }

    /// Process one queued item through the full pipeline, with automatic
    /// retries on failure (the agent never sees these — write-failure
    /// handling lives entirely server-side). Up to `INGEST_MAX_RETRIES`
    /// attempts within `INGEST_DEADLINE`; a retry after a partial success
    /// is self-healing because the W2 dedup gates NOOP already-written facts.
    async fn process_one_pending(&self, node: &PendingNode) -> bool {
        if self
            .set_pending_status(&node.pending_id, STATUS_PROCESSING, "", "")
            .await
            .is_err()
        {
            warn!(
                "Ingest worker: failed to mark {} processing",
                node.pending_id
            );
            return false;
        }

        let agent = (!node.agent_id.is_empty()).then_some(node.agent_id.as_str());
        let tags = (!node.context_tags.is_empty()).then_some(node.context_tags.as_str());

        let deadline = std::time::Instant::now() + INGEST_DEADLINE;
        let mut last_err = String::new();
        let mut attempt = 0u32;
        let outcome = loop {
            attempt += 1;
            match self
                .add_memory(&node.raw_message, &node.user_id, agent, None, tags)
                .await
            {
                Ok(result) => break Some(result),
                Err(e) => {
                    last_err = e.to_string();
                    warn!(
                        "Ingest worker: {} attempt {attempt}/{INGEST_MAX_RETRIES} failed: {last_err}",
                        node.pending_id
                    );
                    if attempt >= INGEST_MAX_RETRIES || std::time::Instant::now() >= deadline {
                        break None;
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        };

        match outcome {
            Some(result) => {
                let payload = serde_json::json!({
                    "memories_added": result.added.len(),
                    "memory_ids": result.added,
                    "chunks_created": result.chunks_created,
                    "entities_extracted": result.entities_extracted,
                    "relations_created": result.reasoning_relations_created,
                    "needs_clarification": result.needs_clarification,
                });
                let _ = self
                    .set_pending_status(&node.pending_id, STATUS_DONE, &payload.to_string(), "")
                    .await;
                // Outbox (прихожая): deliver the outcome to the user's queue,
                // which the agent drains at session start. The payload doubles
                // as the add_result and carries any charter escalations.
                self.enqueue_notice(&node.user_id, "add_result", &payload, &node.pending_id)
                    .await;
                debug!("Ingest worker: {} done", node.pending_id);
                true
            }
            None => {
                error!(
                    "Ingest worker: {} failed after {INGEST_MAX_RETRIES} attempts: {last_err}",
                    node.pending_id
                );
                let _ = self
                    .set_pending_status(&node.pending_id, STATUS_FAILED, "", &last_err)
                    .await;
                // The outbox carries the failed input back so the agent can
                // decide to retry — "here is what I tried to write".
                let payload = serde_json::json!({
                    "error": last_err,
                    "raw_message": node.raw_message,
                    "retry_hint": "the write failed after automatic retries; re-add if still wanted",
                });
                self.enqueue_notice(&node.user_id, "add_failed", &payload, &node.pending_id)
                    .await;
                false
            }
        }
    }

    /// Write one item to the outbox (прихожая). Best-effort: a delivery that
    /// fails to persist is logged, never fatal to the worker.
    async fn enqueue_notice(
        &self,
        user_id: &str,
        kind: &str,
        payload: &serde_json::Value,
        pending_id: &str,
    ) {
        let notice_id = format!("nt_{}", Uuid::new_v4().simple());
        let params = serde_json::json!({
            "notice_id": notice_id,
            "user_id": user_id,
            "kind": kind,
            "payload": payload.to_string(),
            "pending_id": pending_id,
            "created_at": chrono::Utc::now().to_rfc3339(),
        });
        if let Err(e) = self
            .db
            .execute_query::<serde_json::Value, _>("enqueueNotice", &params)
            .await
        {
            warn!("Outbox: failed to enqueue notice for {user_id}: {e}");
        }
    }

    /// Drain the user's outbox — what happened while the agent was away
    /// (completed adds, escalations). Mirrors `search_incomplete_thoughts`.
    /// Marks each returned notice delivered and prunes its done `PendingInput`
    /// (the queue tombstone — not knowledge, so removing it keeps the invariant
    /// "never delete a Memory" intact).
    pub async fn drain_notices(
        &self,
        user_id: &str,
        limit: usize,
    ) -> Result<Vec<MemoryNotice>, ToolingError> {
        let params = serde_json::json!({ "user_id": user_id, "limit": limit as i64 });
        let resp: NoticeList = self
            .db
            .execute_query("getUndeliveredNotices", &params)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        let mut out = Vec::with_capacity(resp.notices.len());
        for n in resp.notices {
            // Mark delivered, then prune the tombstone PendingInput.
            let _ = self
                .db
                .execute_query::<serde_json::Value, _>(
                    "markNoticeDelivered",
                    &serde_json::json!({ "notice_id": n.notice_id }),
                )
                .await;
            if !n.pending_id.is_empty() {
                let _ = self
                    .db
                    .execute_query::<serde_json::Value, _>(
                        "deletePendingInput",
                        &serde_json::json!({ "pending_id": n.pending_id }),
                    )
                    .await;
            }
            out.push(MemoryNotice {
                notice_id: n.notice_id,
                kind: n.kind,
                payload: serde_json::from_str(&n.payload).unwrap_or(serde_json::Value::Null),
                pending_id: n.pending_id,
                created_at: n.created_at,
            });
        }
        Ok(out)
    }

    /// Drains the whole queue once, serially. Returns how many items were
    /// processed. For deterministic tests and one-shot batch tools.
    pub async fn drain_pending_once(&self) -> usize {
        let mut processed = 0;
        match self.fetch_pending_batch(256).await {
            Ok(mut batch) if !batch.is_empty() => {
                batch.sort_by(|a, b| a.created_at.cmp(&b.created_at));
                for node in batch {
                    self.process_one_pending(&node).await;
                    processed += 1;
                }
            }
            _ => {}
        }
        processed
    }
}

/// The single serial worker. Spawned once at startup when the buffer is on.
/// Drains `pending` items oldest-first, one at a time — serialization is the
/// whole point (dedup-race closure), so this never parallelizes.
pub async fn run_ingest_worker(tm: Arc<ToolingManager>) {
    let interval = poll_interval();
    info!(
        "Ingest worker started (poll {}ms); add_memory now returns pending_id",
        interval.as_millis()
    );

    loop {
        match tm.fetch_pending_batch(32).await {
            Ok(mut batch) if !batch.is_empty() => {
                // Oldest first — fairness and causal order for dedup.
                batch.sort_by(|a, b| a.created_at.cmp(&b.created_at));
                for node in batch {
                    tm.process_one_pending(&node).await;
                }
            }
            Ok(_) => {
                tokio::time::sleep(interval).await;
            }
            Err(e) => {
                warn!("Ingest worker: queue poll failed ({e}); backing off");
                tokio::time::sleep(interval.max(Duration::from_secs(2))).await;
            }
        }
    }
}
