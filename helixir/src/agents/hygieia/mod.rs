//! Hygieia — the health watchdog (the 2026-07-02 OOM incident, made an organ).
//!
//! The Moirai generate; Hygieia keeps the organism alive while they do.
//! Detectors sample the substrate (DB liveness, container memory, insight
//! flood, orphaned daemons) and reactions climb a ladder:
//!
//! 1. **Self-heal silently** — pause a flooding insights stage, restart a
//!    dead database container (config-gated) — the user never notices;
//! 2. **Alert through the memory itself** — an `ops_alert` notice lands in
//!    every configured user's outbox (delivered in `pending_outcomes` on
//!    their next write) plus an `ops-alert` memory under `helixir`, so the
//!    incident is recallable knowledge, not a lost log line;
//! 3. **Journal everything** — append-only `health.jsonl`, viewable with
//!    `helixir health`.
//!
//! Two hosts run her: a side-check inside the Moirai daemon's pass loop, and
//! the standalone `helixir watch` service for setups with no daemon.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::toolkit::tooling_manager::ToolingManager;

/// One journaled health event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthEvent {
    pub at: String,
    /// `ok` | `alert` | `heal`
    pub severity: String,
    /// Detector or action name: `db_down`, `mem_pressure`, `insight_flood`,
    /// `orphan_daemon`, `container_restarted`, `insights_paused`, ...
    pub kind: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub detail: serde_json::Value,
}

/// Journal path: `$HELIXIR_HEALTH_LOG` or `~/.helixir/health.jsonl`.
pub fn journal_path() -> PathBuf {
    if let Ok(p) = std::env::var("HELIXIR_HEALTH_LOG") {
        return PathBuf::from(p);
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        .join(".helixir")
        .join("health.jsonl")
}

/// Append one event to the health journal. Best-effort: health reporting must
/// never take the patient down with it.
pub fn journal(event: &HealthEvent) {
    let path = journal_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_string(event) {
        Ok(line) => {
            use std::io::Write;
            if let Err(e) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .and_then(|mut f| writeln!(f, "{line}"))
            {
                warn!("hygieia: journal append failed: {e}");
            }
        }
        Err(e) => warn!("hygieia: journal serialize failed: {e}"),
    }
}

/// The insight-flood brake: N CONSECUTIVE passes that hit the Atropos persist
/// cap mean routing keeps re-finding the same drifting threads — pause the
/// insights stage instead of grinding the substrate (53 passes / 173
/// near-duplicates / two kernel OOM kills taught us this).
#[derive(Debug, Default)]
pub struct FloodTracker {
    consecutive_capped: u32,
    paused: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FloodVerdict {
    Ok,
    /// Cap was hit this pass; not yet actionable.
    Capped(u32),
    /// Threshold reached — the caller must pause the insights stage.
    PauseInsights,
}

impl FloodTracker {
    /// `persisted` from the pass vs the Atropos per-pass cap.
    pub fn observe(&mut self, persisted: usize, cap: usize, passes_to_pause: u32) -> FloodVerdict {
        if self.paused {
            return FloodVerdict::Ok;
        }
        if cap > 0 && persisted >= cap {
            self.consecutive_capped += 1;
            if self.consecutive_capped >= passes_to_pause {
                self.paused = true;
                return FloodVerdict::PauseInsights;
            }
            return FloodVerdict::Capped(self.consecutive_capped);
        }
        self.consecutive_capped = 0;
        FloodVerdict::Ok
    }
}

/// Parsed `docker stats` sample for one container.
#[derive(Debug, Clone, PartialEq)]
pub struct MemSample {
    pub used_mib: f64,
    pub limit_mib: f64,
}

impl MemSample {
    pub fn pct(&self) -> f64 {
        if self.limit_mib <= 0.0 {
            return 0.0;
        }
        self.used_mib / self.limit_mib * 100.0
    }
}

/// Parse a docker `{{.MemUsage}}` cell like `"557.3MiB / 3GiB"`.
pub fn parse_mem_usage(cell: &str) -> Option<MemSample> {
    let (used, limit) = cell.split_once('/')?;
    Some(MemSample {
        used_mib: parse_size_mib(used.trim())?,
        limit_mib: parse_size_mib(limit.trim())?,
    })
}

fn parse_size_mib(s: &str) -> Option<f64> {
    let (num, unit) = s.split_at(s.find(|c: char| c.is_ascii_alphabetic())?);
    let v: f64 = num.trim().parse().ok()?;
    Some(match unit.trim() {
        "KiB" | "kB" | "KB" => v / 1024.0,
        "MiB" | "MB" => v,
        "GiB" | "GB" => v * 1024.0,
        "B" => v / (1024.0 * 1024.0),
        _ => return None,
    })
}

mod backup;
mod monitor;
mod persistence;
mod runtime;

pub use backup::{newest_backup_age_hours, prune_backups};
pub use monitor::Hygieia;
pub use runtime::orphan_daemon;
use runtime::{reclaim_container_cache, restart_container, sample_container_memory};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "mod_backup_tests.rs"]
mod backup_tests;
