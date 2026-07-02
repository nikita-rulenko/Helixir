//! The Moira daemon (#42) — the runtime that schedules the orchestrator.
//!
//! Choreography (what sequence) is the orchestrator's; this owns scheduling
//! (when). It runs `full_pass` either continuously (a loop on an interval, with
//! graceful Ctrl-C shutdown) or on-call (a single pass), turning the Moirai from
//! "driven by hand through the CLL" into genuine background processes.
//!
//! v0 is single-instance. Per the design, at scale each Moira runs as **N
//! parallel instances** (sharded, idempotent, Ractor-supervised) since memory
//! only grows — that supervision tree is the next increment; this is the loop it
//! will manage.

use std::time::Duration;

use tracing::{info, warn};

use crate::agents::atropos::Atropos;
use crate::agents::orchestrator::{Orchestrator, PassConfig, PipelineRun};
use crate::toolkit::tooling_manager::ToolingManager;
use crate::toolkit::tooling_manager::types::ToolingError;

/// Run-mode + cadence for the daemon.
pub struct DaemonConfig {
    pub user: String,
    /// Sleep between passes in continuous mode.
    pub interval: Duration,
    /// On-call: run a single pass and stop.
    pub once: bool,
    /// Host label stamped on this daemon's swarm presence (#39).
    pub host: String,
    pub pass: PassConfig,
    /// Per-stage cadence: run the stage every Nth pass (1 = every pass,
    /// 0 = never). Defaults come from `moira.daemon.*_every_passes` in config;
    /// the CLI flags override per launch.
    pub clotho_every: u64,
    pub insight_every: u64,
    pub merge_every: u64,
    pub reconcile_every: u64,
}

/// The daemon. Borrows the toolkit; constructs the orchestrator per run.
pub struct Daemon<'a> {
    tooling: &'a ToolingManager,
}

impl<'a> Daemon<'a> {
    pub fn new(tooling: &'a ToolingManager) -> Self {
        Self { tooling }
    }

    /// Run the scheduling loop. `on_pass(pass_number, &run)` is the sink — the
    /// caller journals/displays each completed pass. Continuous until Ctrl-C
    /// unless `cfg.once`. A failed pass is logged and the loop continues.
    pub async fn run<F>(&self, cfg: DaemonConfig, mut on_pass: F) -> Result<(), ToolingError>
    where
        F: FnMut(u64, &PipelineRun),
    {
        let orchestrator = Orchestrator::new(self.tooling);
        info!(
            "daemon: starting ({}) for user '{}'",
            if cfg.once {
                "on-call".into()
            } else {
                format!("continuous, every {}s", cfg.interval.as_secs())
            },
            cfg.user
        );

        // The daemon is an agent in the swarm — it announces presence in the
        // shared graph each pass so any host's roster (`helixir swarm`) sees it.
        let agent_id = format!("daemon:{}", cfg.user);

        // Per-stage cadence: a stage runs on the passes where
        // (pass-1) % every == 0, so everything due fires on pass 1;
        // every = 0 disables the stage entirely.
        let due = |every: u64, pass: u64| every != 0 && (pass - 1) % every == 0;
        info!(
            "daemon cadence: clotho every {} pass(es), insight stage every {}, merge every {}, reconcile every {}",
            cfg.clotho_every, cfg.insight_every, cfg.merge_every, cfg.reconcile_every
        );

        // Hygieia rides along: flood brake + substrate checks each pass. The
        // insights cadence is a LOCAL variable so her flood verdict can zero
        // it for this daemon's lifetime without touching config.
        let mut hygieia = crate::agents::hygieia::Hygieia::new(self.tooling);
        let mut flood = crate::agents::hygieia::FloodTracker::default();
        let mut insight_every = cfg.insight_every;
        let atropos_cap = self.tooling.config.moira.atropos.max_persist_per_pass;
        let flood_bar = self.tooling.config.watchdog.flood_passes_to_pause;
        let watchdog_on = self.tooling.config.watchdog.enabled;

        let mut pass = 0u64;
        loop {
            pass += 1;
            if let Err(e) = self
                .tooling
                .register_or_heartbeat(&agent_id, "daemon", &cfg.host, "working")
                .await
            {
                warn!("daemon: heartbeat failed (pass {pass}): {e}");
            }
            let mut pass_cfg = cfg.pass.clone();
            pass_cfg.run_clotho = due(cfg.clotho_every, pass);
            pass_cfg.run_insights = due(insight_every, pass);
            if pass_cfg.run_clotho || pass_cfg.run_insights {
                match orchestrator.full_pass(&cfg.user, &pass_cfg).await {
                    Ok(run) => {
                        if watchdog_on && pass_cfg.run_insights {
                            use crate::agents::hygieia::FloodVerdict;
                            match flood.observe(run.persisted, atropos_cap, flood_bar) {
                                FloodVerdict::PauseInsights => {
                                    insight_every = 0;
                                    crate::agents::hygieia::journal(
                                        &crate::agents::hygieia::HealthEvent {
                                            at: chrono::Utc::now().to_rfc3339(),
                                            severity: "heal".into(),
                                            kind: "insights_paused".into(),
                                            summary: format!(
                                                "insights stage paused after {flood_bar} consecutive capped passes (pass {pass})"
                                            ),
                                            detail: serde_json::Value::Null,
                                        },
                                    );
                                    hygieia
                                        .alert(
                                            "insight_flood",
                                            &format!(
                                                "Atropos hit the persist cap {flood_bar} passes in a row — the insights stage is PAUSED for this daemon's lifetime. Restart the daemon to resume; consider whether the corpus is drifting or the cap is too low."
                                            ),
                                            serde_json::json!({"pass": pass, "cap": atropos_cap}),
                                        )
                                        .await;
                                }
                                FloodVerdict::Capped(n) => {
                                    info!(
                                        "daemon: persist cap hit ({n}/{flood_bar} toward flood pause)"
                                    );
                                }
                                FloodVerdict::Ok => {}
                            }
                        }
                        on_pass(pass, &run);
                    }
                    Err(e) => warn!("daemon: pass {pass} failed: {e}"),
                }
            }

            // Substrate vitals once per pass (cheap; alerts are cooldown-deduped).
            if watchdog_on {
                hygieia.check_db().await;
                hygieia.check_memory().await;
                hygieia.run_backup_duty().await;
            }

            // Drain contradiction debt — keep resolved=0 cross-user disputes
            // from piling up as the collective grows (#45).
            if due(cfg.reconcile_every, pass) {
                let debt_limit = self.tooling.config.moira.daemon.reconcile_limit;
                match Atropos::new(self.tooling)
                    .reconcile(&cfg.user, debt_limit)
                    .await
                {
                    Ok(s) if s.scanned > 0 => info!(
                        "daemon: reconciled debt — drained {} pref + {} superseded, {} live kept ({} surfaced)",
                        s.drained_preference, s.drained_superseded, s.kept_live, s.notified
                    ),
                    Ok(_) => {}
                    Err(e) => warn!("daemon: reconcile failed: {e}"),
                }
            }

            // Paraphrase backstop (#43/#55) — merge same-meaning facts into one
            // fingerprint group, NLI-gated (never merges a contradiction). Runs
            // on its own cadence when the local NLI model is installed
            // (collective/insights). Compiled out without the `nli` feature.
            #[cfg(feature = "nli")]
            if due(cfg.merge_every, pass) && crate::llm::nli::status().installed {
                let mlim = self.tooling.config.moira.daemon.merge_limit;
                let mcos = self.tooling.config.moira.daemon.merge_cosine_threshold;
                match Atropos::new(self.tooling)
                    .merge_paraphrases(mlim, mcos)
                    .await
                {
                    Ok(s) if s.merged_groups > 0 => info!(
                        "daemon: merged {} paraphrase group(s) ({} nodes); {} contradictions blocked",
                        s.merged_groups, s.nodes_restamped, s.contradictions_blocked
                    ),
                    Ok(_) => {}
                    Err(e) => warn!("daemon: paraphrase merge failed: {e}"),
                }
            }

            if cfg.once {
                break;
            }
            // Idle heartbeat before sleeping — the roster shows live-but-idle
            // rather than going stale the instant a pass finishes.
            let _ = self
                .tooling
                .register_or_heartbeat(&agent_id, "daemon", &cfg.host, "idle")
                .await;
            tokio::select! {
                _ = tokio::time::sleep(cfg.interval) => {}
                _ = tokio::signal::ctrl_c() => {
                    info!("daemon: shutdown signal, stopping after pass {pass}");
                    break;
                }
            }
        }
        Ok(())
    }
}
