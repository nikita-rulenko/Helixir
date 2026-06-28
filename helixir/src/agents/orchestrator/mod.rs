//! The orchestrator — choreography for the Moirai (#41 / Moira).
//!
//! The three agents are proven individually; this prescribes the SCENARIO they
//! move through — Clotho tags → Lachesis routes & gates → Atropos curates — as a
//! single pass. It composes agents (the top of the agents layer); WHEN a pass
//! runs is a separate concern (the daemon's, #42). Keeping choreography (what
//! sequence) apart from scheduling (when) is the point.

use tracing::info;

use crate::agents::atropos::{Atropos, Insight};
use crate::agents::clotho::{Clotho, GrowStats};
use crate::toolkit::tooling_manager::ToolingManager;
use crate::toolkit::tooling_manager::types::ToolingError;

/// What one full pass produced — Clotho's tagging stats and Atropos's insights.
#[derive(Debug)]
pub struct PipelineRun {
    pub grow: GrowStats,
    pub insights: Vec<Insight>,
}

/// Tunables for a pass. Defaults match the CLI.
pub struct PassConfig {
    pub corpus_limit: i64,
    pub grow_threshold: f64,
    pub max_seeds: usize,
    pub max_hops: usize,
}

impl Default for PassConfig {
    fn default() -> Self {
        // Single source of truth — the orchestrator defaults live in config.
        Self::from_config(&crate::core::config::HelixirConfig::default())
    }
}

impl PassConfig {
    /// Source the pass tunables from config (respects helixir.toml / env).
    pub fn from_config(c: &crate::core::config::HelixirConfig) -> Self {
        let o = &c.moira.orchestrator;
        Self {
            corpus_limit: o.corpus_limit as i64,
            grow_threshold: o.grow_threshold,
            max_seeds: o.max_seeds,
            max_hops: o.max_hops,
        }
    }
}

/// Orchestrates the Moirai. Borrows the toolkit; constructs the agents per pass.
pub struct Orchestrator<'a> {
    tooling: &'a ToolingManager,
}

impl<'a> Orchestrator<'a> {
    pub fn new(tooling: &'a ToolingManager) -> Self {
        Self { tooling }
    }

    /// The canonical scenario over `user`'s memories: Clotho seeds + grows the
    /// dictionary and tags the corpus, then Atropos curates the subset graph
    /// into ranked, deduped insights (Lachesis routes & gates inside Atropos).
    /// One end-to-end choreography, returned + ready to journal.
    pub async fn full_pass(
        &self,
        user: &str,
        cfg: &PassConfig,
    ) -> Result<PipelineRun, ToolingError> {
        // 1) Clotho — tag (and grow the dictionary on misses).
        let clotho = Clotho::new(self.tooling);
        clotho.seed_dictionary().await?;
        let mems = self
            .tooling
            .list_user_memories(user, cfg.corpus_limit)
            .await?;
        let grow = clotho.grow_pass(&mems, cfg.grow_threshold).await?;

        // 2) Atropos — curate the woven subsets (Lachesis routes & gates within).
        let orch = &self.tooling.config.moira.orchestrator;
        let candidates = self.tooling.list_categories(orch.candidate_cap).await?;
        let universe = self
            .tooling
            .total_memory_count(orch.universe_cap)
            .await?
            .max(1);
        let seeds: Vec<(String, String)> = candidates.iter().take(cfg.max_seeds).cloned().collect();
        let atropos = Atropos::new(self.tooling);
        let insights = atropos
            .curate(&seeds, &candidates, universe, cfg.max_hops)
            .await?;

        info!(
            "orchestrator.full_pass(user={user}): tagged {} (minted {}), {} insights",
            grow.tagged_by_match + grow.reused_mint,
            grow.minted,
            insights.len()
        );
        Ok(PipelineRun { grow, insights })
    }
}
