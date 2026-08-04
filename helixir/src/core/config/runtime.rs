//! Runtime, background-worker, and transport configuration.

use super::*;

/// Write-path (add pipeline) policy values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteConfig {
    pub recall_top_k: usize,
    pub cross_user_dedup_top_k: usize,
    pub cross_user_link_certainty: i64,
    pub cross_user_default_conflict_type: String,
    pub contradict_edge_strength: i64,
    pub entity_link_strength: i64,
    pub entity_link_confidence: i64,
    pub relation_inference_context_k: usize,
    pub raw_source_certainty: u8,
    pub raw_source_importance: u8,
    pub raw_source_min_chars: usize,
    /// Strength of the atom→raw PART_OF edge written when a raw source is
    /// stored (#82): the family link that lets search collapse a raw and its
    /// atoms into one result instead of billing the same content twice.
    pub raw_part_of_strength: i32,
    pub fallback_certainty: u8,
    pub fallback_importance: u8,
    pub context_link_priority: i64,
    /// Charter C5: confidence below which a rewrite is escalated to the human.
    pub charter_low_confidence: u8,
    /// #34 increment 2b: after this many IDENTICAL contradiction-review
    /// verdicts (same new-type/old-type/strategy shape), resolve_contradiction
    /// proposes a standing rule. 0 disables precedent learning.
    pub rule_propose_after: usize,
    /// #96 Lever 2: route SUPPORTS/CONTRADICTS relation inference through the
    /// required local NLI judge instead of the LLM. A missing/corrupt model
    /// keeps relation inference on the LLM, while onboarding/doctor report the
    /// installation as unready.
    pub nli_route: bool,
    /// Minimum NLI softmax probability for a routed edge; unconfident pairs
    /// stay with the LLM.
    pub nli_route_min_prob: f32,
    /// Charter increment 2 (#34): when a destructive verdict (UPDATE /
    /// SUPERSEDE / DELETE) hits a charter escalation, DEFER it instead of
    /// executing — store the new fact alongside the old, record a
    /// charter_deferred CONTRADICTS edge, and let the agent settle it with
    /// resolve_contradiction (retract = the supersede happens then).
    pub charter_blocking: bool,
}
impl Default for WriteConfig {
    fn default() -> Self {
        Self {
            recall_top_k: 5,
            cross_user_dedup_top_k: 5,
            cross_user_link_certainty: 80,
            cross_user_default_conflict_type: "preference".to_string(),
            contradict_edge_strength: 80,
            entity_link_strength: 80,
            entity_link_confidence: 50,
            relation_inference_context_k: 5,
            raw_source_certainty: 70,
            raw_source_importance: 40,
            raw_source_min_chars: 100,
            raw_part_of_strength: 80,
            fallback_certainty: 50,
            fallback_importance: 50,
            context_link_priority: 50,
            charter_low_confidence: 70,
            rule_propose_after: 3,
            nli_route: true,
            nli_route_min_prob: 0.85,
            charter_blocking: true,
        }
    }
}

/// Ingest buffer (#25) durability/latency.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IngestConfig {
    pub max_retries: u32,
    pub deadline_secs: u64,
    pub poll_interval_ms: u64,
    pub drain_batch_size: usize,
    pub retry_backoff_ms: u64,
    pub worker_batch_size: usize,
    /// Confirm-or-promise window (#63). When the buffer is on, `add_memory`
    /// waits up to this long for THIS write to finish so the agent gets a real
    /// result with memory_ids instead of a bare "pending" it misreads as
    /// failure. On timeout it returns an explicit "accepted" success ack.
    pub ack_wait_ms: u64,
    /// Poll cadence for the confirm-or-promise wait above.
    pub ack_poll_ms: u64,
}
impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            deadline_secs: 60,
            poll_interval_ms: 500,
            drain_batch_size: 256,
            retry_backoff_ms: 500,
            worker_batch_size: 32,
            ack_wait_ms: 8000,
            ack_poll_ms: 150,
        }
    }
}

/// Text chunking (long inputs are split before embedding/storage).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChunkingConfig {
    /// Inputs longer than this many characters are chunked.
    pub threshold: usize,
    /// Target chunk size (characters).
    pub chunk_size: usize,
}
impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            threshold: 500,
            chunk_size: 512,
        }
    }
}

/// Swarm rendezvous (#39) presence defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SwarmConfig {
    pub active_window_secs: u64,
    /// #84: agents silent longer than this are hidden from swarm_status
    /// (presumed gone — one-shots never say goodbye). Deliberately larger
    /// than the daemon pass interval (~600s) so healthy daemons don't flap.
    /// 0 disables hiding. The Agent node itself is never deleted: it anchors
    /// AGENT_CREATED authorship provenance.
    pub presence_ttl_secs: u64,
    pub default_role: String,
    pub default_status: String,
}
impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            active_window_secs: 90,
            presence_ttl_secs: 1800,
            default_role: "developer".to_string(),
            default_status: "idle".to_string(),
        }
    }
}

/// Gateway (#42) serving defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    pub default_bind: String,
    /// Optional bearer token for the HTTP MCP gateway. `None` preserves the
    /// intentional full-trust network model. Prefer the
    /// `HELIXIR_GATEWAY_TOKEN` environment override for secrets.
    pub auth_token: Option<String>,
}
impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            default_bind: "0.0.0.0:8765".to_string(),
            auth_token: None,
        }
    }
}

/// LLM/embedding runtime knobs that were previously hardcoded at provider
/// construction (ollama request timeout, embedding cache sizing).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmRuntimeConfig {
    /// Ollama HTTP request timeout (seconds).
    pub request_timeout_secs: u64,
    /// Embedding cache capacity (entries).
    pub embedding_cache_size: usize,
    /// Embedding cache entry TTL (seconds).
    pub embedding_cache_ttl_secs: u64,
}
impl Default for LlmRuntimeConfig {
    fn default() -> Self {
        Self {
            request_timeout_secs: crate::DEFAULT_LLM_REQUEST_TIMEOUT_SECS,
            embedding_cache_size: crate::DEFAULT_CACHE_SIZE,
            embedding_cache_ttl_secs: crate::DEFAULT_CACHE_TTL,
        }
    }
}

/// FastThink (think_* tools) session limits. Defaults match the MCP preset
/// (`FastThinkLimits::mcp`) — the profile the live server runs with.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FastThinkConfig {
    pub max_thoughts: usize,
    pub max_entities: usize,
    pub max_concepts: usize,
    pub max_depth: usize,
    pub thinking_timeout_secs: u64,
    pub session_ttl_secs: u64,
    pub max_recall_results: usize,
    /// Fast-commit ceiling: conclusions up to this many chars skip the
    /// re-extraction LLM call on `think_commit` (the session already IS the
    /// structure). Longer blobs fall back to full extraction — atomizing a
    /// wall of text is worth the wait.
    pub commit_extract_over_chars: usize,
    /// certainty (0-100) stamped on fast-committed conclusions.
    pub commit_certainty: u32,
    /// importance (0-100) stamped on fast-committed conclusions.
    pub commit_importance: u32,
    /// Strength of the SUPPORTS provenance edge written from each recalled
    /// evidence memory to the committed conclusion.
    pub commit_support_strength: u32,
    /// Score floor for think_recall: rows below this combined score never
    /// enter the session, even inside the top-K. Measured on the live store
    /// (#81): seeds sit at 0.68–0.99, the graph-expansion tail flattens at
    /// 0.41–0.55, and the knee lands at 0.60–0.65 on every query class —
    /// the floor exists for THIN stores, where the top-K would otherwise
    /// reach down into that noise floor. 0.0 disables.
    pub recall_min_score: f32,
    /// #90: when the primary recall pass returns ZERO rows, one fallback
    /// pass runs in `full` mode with this relaxed floor. The belt's failure
    /// mode must not be a silent zero — a weak model reads that as "no
    /// evidence exists" and reasons unsupported.
    pub recall_fallback_min_score: f32,
    /// #90: hard cap on fallback rows (smaller than max_recall_results —
    /// weak evidence never floods the tree). 0 disables the fallback.
    pub recall_fallback_max: usize,
    /// #78: think_recall stops this many slots short of the thought cap, so
    /// the agent can always add a synthesis thought and conclude — recalled
    /// evidence must never trap the session at the cap.
    pub conclude_reserve: usize,
}
impl Default for FastThinkConfig {
    fn default() -> Self {
        Self {
            max_thoughts: 150,
            max_entities: 80,
            max_concepts: 40,
            max_depth: 12,
            thinking_timeout_secs: 90,
            session_ttl_secs: 600,
            max_recall_results: 8,
            commit_extract_over_chars: 900,
            commit_certainty: 75,
            commit_importance: 60,
            commit_support_strength: 60,
            recall_min_score: 0.6,
            recall_fallback_min_score: 0.45,
            recall_fallback_max: 3,
            conclude_reserve: 2,
        }
    }
}

/// Hygieia — the built-in health watchdog (the OOM incident of 2026-07-02
/// institutionalized). Detectors sample the substrate; reactions climb a
/// ladder: self-heal silently → alert through the memory itself (ops_alert
/// notices + an ops-alert memory) → journal everything to health.jsonl.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchdogConfig {
    pub enabled: bool,
    /// Standalone `helixir watch` sampling period.
    pub sample_interval_secs: u64,
    /// Docker container to watch/heal. Empty disables the memory detector
    /// and the restart heal (HTTP liveness probes still run).
    pub container_name: String,
    /// Alert when container memory crosses this % of its limit.
    pub mem_alert_pct: f64,
    /// Allow `docker restart <container_name>` as a self-heal for a dead or
    /// near-cap database. Conservative default: off.
    pub allow_container_restart: bool,
    /// #89: when the memory detector fires, first try shedding reclaimable
    /// page cache via cgroup `memory.reclaim` (a short-lived privileged
    /// helper container — the docker-stats number counts cache as usage).
    /// Only pressure that SURVIVES the reclaim alerts as real. Off by
    /// default: it spawns a privileged container, the operator opts in.
    pub allow_cache_reclaim: bool,
    /// How much to ask the kernel to reclaim per valve opening, in MiB.
    pub reclaim_step_mib: u64,
    /// #89: live-heap pressure at or past this % of the container limit
    /// triggers a supervised `docker restart` INSTEAD of waiting for the
    /// OOM killer (in-process retention reaches the cap in ~a day under
    /// heavy write churn; a restart resets it and preserves the volume).
    /// Requires `allow_container_restart` too. Only the number that
    /// SURVIVES a cache reclaim counts when the valve is enabled.
    /// 0 disables. Should sit above `mem_alert_pct`.
    pub mem_restart_pct: f64,
    /// Insight-flood brake: this many CONSECUTIVE passes hitting the Atropos
    /// persist cap pauses the insights stage for the daemon's lifetime.
    pub flood_passes_to_pause: u32,
    /// A daemon still heartbeating while every other agent has been silent
    /// this long is flagged as likely orphaned.
    pub orphan_daemon_hours: f64,
    /// Who receives ops_alert notices (delivered in pending_outcomes on
    /// their next write — the memory alerts through the memory).
    pub alert_users: Vec<String>,
    /// Re-alerting the same kind is suppressed for this long.
    pub alert_cooldown_secs: u64,
    /// #75: shell command executed on every alert (after journal + memory
    /// notices) — osascript notification, curl to a webhook, anything. The
    /// alert's kind and summary arrive in HELIXIR_ALERT_KIND /
    /// HELIXIR_ALERT_SUMMARY env vars. Empty disables. Best-effort:
    /// failures are logged, never block the alert path.
    pub on_alert_cmd: String,
    /// Autobackup duty (#65): tar the data dir on a schedule. Empty source
    /// disables the duty. When `container_name` is set the container is
    /// paused for the copy (a consistent LMDB snapshot), then unpaused.
    pub backup_source_dir: String,
    /// Where archives land. Default: ~/.helixir/backups.
    pub backup_dir: String,
    pub backup_interval_hours: f64,
    /// Newest N archives survive pruning.
    pub backup_keep: usize,
}
impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sample_interval_secs: 60,
            container_name: String::new(),
            mem_alert_pct: 80.0,
            allow_container_restart: false,
            allow_cache_reclaim: false,
            reclaim_step_mib: 1024,
            mem_restart_pct: 92.0,
            flood_passes_to_pause: 3,
            orphan_daemon_hours: 6.0,
            alert_users: vec!["helixir".to_string()],
            alert_cooldown_secs: 21_600,
            on_alert_cmd: String::new(),
            backup_source_dir: String::new(),
            backup_dir: String::new(),
            backup_interval_hours: 24.0,
            backup_keep: 7,
        }
    }
}
