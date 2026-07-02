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

/// Hygieia herself. Borrows the toolkit like every agent; keeps per-kind
/// alert cooldowns in-process (both hosts are long-lived loops).
pub struct Hygieia<'a> {
    tooling: &'a ToolingManager,
    last_alert: std::collections::HashMap<String, Instant>,
}

impl<'a> Hygieia<'a> {
    pub fn new(tooling: &'a ToolingManager) -> Self {
        Self {
            tooling,
            last_alert: std::collections::HashMap::new(),
        }
    }

    fn cfg(&self) -> &crate::core::config::WatchdogConfig {
        &self.tooling.config.watchdog
    }

    /// The alert ladder, step 2: journal + ops_alert notice to every
    /// configured user + a recallable ops-alert memory under `helixir`.
    /// Cooldown-deduped per kind. Best-effort end to end.
    pub async fn alert(&mut self, kind: &str, summary: &str, detail: serde_json::Value) {
        let cooldown = Duration::from_secs(self.cfg().alert_cooldown_secs);
        if let Some(t) = self.last_alert.get(kind) {
            if t.elapsed() < cooldown {
                return;
            }
        }
        self.last_alert.insert(kind.to_string(), Instant::now());

        warn!("HYGIEIA ALERT [{kind}]: {summary}");
        journal(&HealthEvent {
            at: chrono::Utc::now().to_rfc3339(),
            severity: "alert".into(),
            kind: kind.into(),
            summary: summary.into(),
            detail: detail.clone(),
        });

        let payload = serde_json::json!({
            "kind": kind,
            "summary": summary,
            "detail": detail,
            "runbook": "helixir health — recent events; the journal is ~/.helixir/health.jsonl",
        });
        for user in self.cfg().alert_users.clone() {
            self.tooling
                .enqueue_notice(&user, "ops_alert", &payload, "")
                .await;
        }

        // A recallable trace: incidents are knowledge. Skipped silently when
        // the embedder is down — the notice + journal already carry the alert.
        let text = format!(
            "OPS ALERT ({kind}) on {}: {summary}",
            chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")
        );
        if let Ok(vector) = self.tooling.embedder.generate(&text, true).await {
            let memory = crate::llm::extractor::ExtractedMemory {
                text,
                memory_type: "fact".to_string(),
                certainty: 90,
                importance: 80,
                entities: vec![],
                context: None,
            };
            if let Err(e) = self
                .tooling
                .store_new_memory(&memory, "helixir", &vector, "ops-alert")
                .await
            {
                warn!("hygieia: ops-alert memory store failed: {e}");
            }
        }
    }

    /// Liveness probe: the cheapest read that exercises the full stack. On
    /// failure, optionally self-heal by restarting the configured container.
    pub async fn check_db(&mut self) -> bool {
        let alive = self
            .tooling
            .db
            .execute_query::<serde_json::Value, _>(
                "getAllCategories",
                &serde_json::json!({"limit": 1}),
            )
            .await
            .is_ok();
        if alive {
            return true;
        }
        let name = self.cfg().container_name.clone();
        if self.cfg().allow_container_restart && !name.is_empty() {
            info!("hygieia: DB down — attempting container restart ({name})");
            let healed = restart_container(&name).await;
            journal(&HealthEvent {
                at: chrono::Utc::now().to_rfc3339(),
                severity: "heal".into(),
                kind: "container_restarted".into(),
                summary: format!(
                    "database was unreachable; docker restart {name} {}",
                    if healed { "succeeded" } else { "FAILED" }
                ),
                detail: serde_json::Value::Null,
            });
            if healed {
                // Alert anyway — a self-heal the operator never learns about
                // becomes a mystery next week.
                self.alert(
                    "db_restarted",
                    &format!("database container {name} was down and was auto-restarted"),
                    serde_json::Value::Null,
                )
                .await;
                return self
                    .tooling
                    .db
                    .execute_query::<serde_json::Value, _>(
                        "getAllCategories",
                        &serde_json::json!({"limit": 1}),
                    )
                    .await
                    .is_ok();
            }
        }
        self.alert(
            "db_down",
            "database liveness probe failed (and no self-heal applied)",
            serde_json::Value::Null,
        )
        .await;
        false
    }

    /// Container memory pressure. No container configured → silently skipped.
    pub async fn check_memory(&mut self) {
        let name = self.cfg().container_name.clone();
        if name.is_empty() {
            return;
        }
        let Some(sample) = sample_container_memory(&name).await else {
            return;
        };
        if sample.pct() >= self.cfg().mem_alert_pct {
            self.alert(
                "mem_pressure",
                &format!(
                    "container {name} at {:.0}% of its memory limit ({:.0}/{:.0} MiB)",
                    sample.pct(),
                    sample.used_mib,
                    sample.limit_mib
                ),
                serde_json::json!({"used_mib": sample.used_mib, "limit_mib": sample.limit_mib}),
            )
            .await;
        }
    }

    /// A daemon still heartbeating while every OTHER agent has been silent
    /// for `orphan_daemon_hours` is probably forgotten — exactly how the OOM
    /// incident started. Alert-only: killing someone's daemon is not ours.
    pub async fn check_orphan_daemons(&mut self) {
        let horizon = (self.cfg().orphan_daemon_hours * 3600.0) as i64;
        let Ok(roster) = self.tooling.list_swarm().await else {
            return;
        };
        let now = chrono::Utc::now();
        if let Some(name) = orphan_daemon(&roster, now, horizon) {
            self.alert(
                "orphan_daemon",
                &format!(
                    "{name} is still running while no other agent has been active for {:.1}h — forgotten after a test? (`helixir daemon stop`)",
                    self.cfg().orphan_daemon_hours
                ),
                serde_json::Value::Null,
            )
            .await;
        }
    }
}

/// Pure orphan policy (unit-testable): a fresh daemon heartbeat + every
/// non-daemon agent silent past the horizon → that daemon's id.
pub fn orphan_daemon(
    roster: &[crate::toolkit::tooling_manager::swarm::AgentPresence],
    now: chrono::DateTime<chrono::Utc>,
    horizon_secs: i64,
) -> Option<String> {
    let daemon = roster
        .iter()
        .find(|a| a.role == "daemon" && a.is_active(now, 1800))?;
    let others_active = roster.iter().any(|a| {
        a.role != "daemon"
            && matches!(a.age_seconds(now), Some(age) if (0..=horizon_secs).contains(&age))
    });
    if others_active {
        None
    } else {
        Some(daemon.agent_id.clone())
    }
}

/// `docker stats` one-shot for a container's memory cell.
pub async fn sample_container_memory(name: &str) -> Option<MemSample> {
    let out = tokio::process::Command::new("docker")
        .args(["stats", "--no-stream", "--format", "{{.MemUsage}}", name])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_mem_usage(String::from_utf8_lossy(&out.stdout).trim())
}

async fn restart_container(name: &str) -> bool {
    tokio::process::Command::new("docker")
        .args(["restart", name])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flood_tracker_pauses_after_consecutive_caps_only() {
        let mut t = FloodTracker::default();
        assert_eq!(t.observe(6, 6, 3), FloodVerdict::Capped(1));
        assert_eq!(t.observe(2, 6, 3), FloodVerdict::Ok, "streak broken");
        assert_eq!(t.observe(6, 6, 3), FloodVerdict::Capped(1));
        assert_eq!(t.observe(6, 6, 3), FloodVerdict::Capped(2));
        assert_eq!(t.observe(6, 6, 3), FloodVerdict::PauseInsights);
        assert_eq!(t.observe(6, 6, 3), FloodVerdict::Ok, "latched: fires once");
    }

    #[test]
    fn mem_usage_cell_parses_docker_units() {
        let s = parse_mem_usage("557.3MiB / 3GiB").unwrap();
        assert!((s.used_mib - 557.3).abs() < 0.01);
        assert!((s.limit_mib - 3072.0).abs() < 0.01);
        assert!((s.pct() - 18.14).abs() < 0.1);
        assert!(parse_mem_usage("garbage").is_none());
    }

    #[test]
    fn orphan_policy_flags_lone_fresh_daemon() {
        use crate::toolkit::tooling_manager::swarm::AgentPresence;
        let now = chrono::Utc::now();
        let mk = |id: &str, role: &str, ago_secs: i64| AgentPresence {
            agent_id: id.into(),
            name: id.into(),
            role: role.into(),
            host: "h".into(),
            last_seen: (now - chrono::Duration::seconds(ago_secs)).to_rfc3339(),
            status: "working".into(),
        };
        // Fresh daemon + stale workers → orphan.
        let roster = vec![
            mk("daemon:claude", "daemon", 30),
            mk("zc-a", "developer", 90_000),
        ];
        assert_eq!(
            orphan_daemon(&roster, now, 6 * 3600),
            Some("daemon:claude".to_string())
        );
        // A recently-active worker clears the suspicion.
        let roster2 = vec![
            mk("daemon:claude", "daemon", 30),
            mk("zc-a", "developer", 600),
        ];
        assert_eq!(orphan_daemon(&roster2, now, 6 * 3600), None);
        // No daemon → nothing to flag.
        let roster3 = vec![mk("zc-a", "developer", 90_000)];
        assert_eq!(orphan_daemon(&roster3, now, 6 * 3600), None);
    }
}
