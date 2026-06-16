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
            match orchestrator.full_pass(&cfg.user, &cfg.pass).await {
                Ok(run) => on_pass(pass, &run),
                Err(e) => warn!("daemon: pass {pass} failed: {e}"),
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
