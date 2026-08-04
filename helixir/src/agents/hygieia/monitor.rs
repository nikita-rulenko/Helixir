//! Watchdog state and health checks.

use super::*;

/// Hygieia herself. Borrows the toolkit like every agent; keeps per-kind
/// alert cooldowns in-process (both hosts are long-lived loops).
pub struct Hygieia<'a> {
    pub(super) tooling: &'a ToolingManager,
    last_alert: std::collections::HashMap<String, Instant>,
}

impl<'a> Hygieia<'a> {
    pub fn new(tooling: &'a ToolingManager) -> Self {
        Self {
            tooling,
            last_alert: std::collections::HashMap::new(),
        }
    }

    pub(super) fn cfg(&self) -> &crate::core::config::WatchdogConfig {
        &self.tooling.config.watchdog
    }

    /// The alert ladder, step 2: journal + ops_alert notice to every
    /// configured user + a recallable ops-alert memory under `helixir`.
    /// Cooldown-deduped per kind. Best-effort end to end.
    pub async fn alert(&mut self, kind: &str, summary: &str, detail: serde_json::Value) {
        let cooldown = Duration::from_secs(self.cfg().alert_cooldown_secs);
        if let Some(t) = self.last_alert.get(kind)
            && t.elapsed() < cooldown
        {
            return;
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

        // #75: the human hook — agents hear alerts through the memory, but a
        // human not currently talking to an agent needs a push (notification,
        // webhook). Fire-and-forget; a hook failure must never block alerts.
        let hook = self.cfg().on_alert_cmd.clone();
        if !hook.is_empty() {
            let (k, s) = (kind.to_string(), summary.to_string());
            tokio::spawn(async move {
                match tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(&hook)
                    .env("HELIXIR_ALERT_KIND", &k)
                    .env("HELIXIR_ALERT_SUMMARY", &s)
                    .output()
                    .await
                {
                    Ok(o) if !o.status.success() => warn!(
                        "on_alert_cmd exited {}: {}",
                        o.status,
                        String::from_utf8_lossy(&o.stderr)
                            .chars()
                            .take(200)
                            .collect::<String>()
                    ),
                    Err(e) => warn!("on_alert_cmd failed to spawn: {e}"),
                    _ => {}
                }
            });
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
                .store_new_memory(&memory, "helixir", &vector, "ops-alert", None)
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
        if sample.pct() < self.cfg().mem_alert_pct {
            return;
        }

        // #89: before crying wolf, open the cache valve. The docker-stats
        // number charges reclaimable page cache to the container (observed
        // live: 2.58GiB reported, ~414MiB true heap); cgroup memory.reclaim
        // sheds exactly the reclaimable part without touching live heap.
        // Only if the number stays high AFTER reclaim is the pressure real.
        let mut live = MemSample {
            used_mib: sample.used_mib,
            limit_mib: sample.limit_mib,
        };
        if self.cfg().allow_cache_reclaim {
            // #89 root cause: the retained pages are the allocator's
            // lazily-freed (MADV_FREE) heap — reclaimable, but only as much
            // as we ASK for. A fixed step under-asked (observed live:
            // "persists after reclaim" that was really "asked 1G of 3G").
            // Ask for the full current charge; reclaim_step_mib stays as
            // the floor for tiny containers.
            let step = self.cfg().reclaim_step_mib.max(sample.used_mib as u64 + 64);
            if reclaim_container_cache(&name, step).await
                && let Some(after) = sample_container_memory(&name).await
            {
                if after.pct() < self.cfg().mem_alert_pct {
                    journal(&HealthEvent {
                        at: chrono::Utc::now().to_rfc3339(),
                        severity: "heal".into(),
                        kind: "cache_reclaimed".into(),
                        summary: format!(
                            "container {name} memory was cache-bloated: {:.0} -> {:.0} MiB after reclaiming up to {step} MiB of page cache",
                            sample.used_mib, after.used_mib
                        ),
                        detail: serde_json::json!({
                            "before_mib": sample.used_mib,
                            "after_mib": after.used_mib,
                            "limit_mib": sample.limit_mib,
                        }),
                    });
                    info!(
                        "hygieia: cache valve — {name} {:.0} -> {:.0} MiB, no real pressure",
                        sample.used_mib, after.used_mib
                    );
                    return;
                }
                // Pressure survived the reclaim: judge the restart bar
                // by the post-reclaim (live-heap) number, not the
                // cache-inflated one.
                live = after;
            }
        }

        // #89: in-process retention reaches the cap in ~a day of heavy write
        // churn, and the OOM killer strikes mid-write. Past the restart bar
        // a supervised restart (volume preserved, ~10s) is the lesser evil.
        // Two opt-ins: the restart permission AND a non-zero bar.
        let restart_pct = self.cfg().mem_restart_pct;
        if restart_pct > 0.0 && live.pct() >= restart_pct && self.cfg().allow_container_restart {
            info!(
                "hygieia: live heap at {:.0}% (>= {restart_pct:.0}%) — supervised restart of {name}",
                live.pct()
            );
            let healed = restart_container(&name).await;
            journal(&HealthEvent {
                at: chrono::Utc::now().to_rfc3339(),
                severity: "heal".into(),
                kind: "mem_restarted".into(),
                summary: format!(
                    "live heap at {:.0}% of limit ({:.0}/{:.0} MiB); docker restart {name} {}",
                    live.pct(),
                    live.used_mib,
                    live.limit_mib,
                    if healed { "succeeded" } else { "FAILED" }
                ),
                detail: serde_json::json!({
                    "live_mib": live.used_mib,
                    "limit_mib": live.limit_mib,
                    "restart_pct": restart_pct,
                }),
            });
            self.alert(
                if healed { "mem_restarted" } else { "mem_pressure" },
                &format!(
                    "container {name} live heap hit {:.0}% of its limit ({:.0}/{:.0} MiB) — auto-restart {}",
                    live.pct(),
                    live.used_mib,
                    live.limit_mib,
                    if healed {
                        "succeeded (heap reset, volume preserved)"
                    } else {
                        "FAILED — intervene before the OOM killer does"
                    }
                ),
                serde_json::json!({"live_mib": live.used_mib, "limit_mib": live.limit_mib}),
            )
            .await;
            return;
        }

        self.alert(
            "mem_pressure",
            &format!(
                "container {name} at {:.0}% of its memory limit ({:.0}/{:.0} MiB){}",
                live.pct(),
                live.used_mib,
                live.limit_mib,
                if self.cfg().allow_cache_reclaim {
                    " — persists after a cache reclaim, this is live heap"
                } else {
                    ""
                }
            ),
            serde_json::json!({"used_mib": live.used_mib, "limit_mib": live.limit_mib}),
        )
        .await;
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
