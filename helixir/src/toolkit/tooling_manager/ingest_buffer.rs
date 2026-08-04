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
use std::sync::OnceLock;
use std::time::Duration;

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::ToolingManager;
use super::types::ToolingError;

/// A completed (or failed) buffered write, broadcast for best-effort MCP
/// push (#25 phase 2). The MCP layer subscribes in `on_initialized` and
/// forwards each event to the client as a logging notification — purely
/// best-effort, the authoritative delivery is the opportunistic outbox.
#[derive(Debug, Clone)]
pub struct NotifyEvent {
    pub user_id: String,
    pub kind: String,
    pub summary: String,
}

/// Process-wide broadcast channel bridging the worker (tooling layer) to the
/// MCP server (which holds the peer). A module-level static avoids threading
/// a sender through every constructor.
fn notify_channel() -> &'static broadcast::Sender<NotifyEvent> {
    static CH: OnceLock<broadcast::Sender<NotifyEvent>> = OnceLock::new();
    CH.get_or_init(|| broadcast::channel(256).0)
}

/// Subscribe to write-completion events (for the MCP push forwarder).
pub fn subscribe_notify() -> broadcast::Receiver<NotifyEvent> {
    notify_channel().subscribe()
}

fn publish_notify(event: NotifyEvent) {
    // Err only means no subscribers — fine, the outbox still has the outcome.
    let _ = notify_channel().send(event);
}

/// A queued input's lifecycle. Stored as the `status` string on the node.
pub const STATUS_PENDING: &str = "pending";
pub const STATUS_PROCESSING: &str = "processing";
pub const STATUS_DONE: &str = "done";
pub const STATUS_FAILED: &str = "failed";

// Server-side auto-retry budget for a queued write (#25). The agent never
// sees these — write-failure handling is entirely internal.
// Ingest retry budget + deadline now live in config.ingest
// (max_retries / deadline_secs / retry_backoff_ms).

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
    /// Authorization metadata is kept server-side and never serialized to the
    /// caller before the RBAC decision has succeeded.
    #[serde(skip)]
    pub owner_id: String,
    #[serde(skip)]
    pub creator_id: String,
    #[serde(skip)]
    pub group_id: String,
}

#[derive(Debug, Deserialize)]
struct PendingNode {
    pending_id: String,
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    actor_id: String,
    #[serde(default)]
    group_id: String,
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
    processed_at: String,
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

mod manager;
mod worker;

pub use worker::run_ingest_worker;
