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
    /// How many insights persist_insights actually stored this pass (after
    /// the flood gate) — Hygieia's flood detector reads this.
    pub persisted: usize,
    /// #91: what the hypothesis verification duty did.
    pub verify: crate::agents::atropos::verify::VerifyStats,
    /// #83: what the retroactive causal stitching stage did.
    pub stitch: crate::agents::lachesis::stitch::StitchStats,
}

/// Tunables for a pass. Defaults match the CLI.
#[derive(Debug, Clone)]
pub struct PassConfig {
    pub corpus_limit: i64,
    pub grow_threshold: f64,
    pub max_seeds: usize,
    pub max_hops: usize,
    /// Stage gates — the daemon flips these per pass to give each Moira its
    /// own cadence (moira.daemon.*_every_passes). A skipped stage contributes
    /// empty stats. Both default to true (a bare full_pass runs everything).
    pub run_clotho: bool,
    pub run_insights: bool,
    /// #83: cadence gate for the retroactive causal stitching stage.
    pub run_stitch: bool,
    /// #91: cadence gate for the hypothesis verification duty.
    pub run_verify: bool,
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
            run_clotho: true,
            run_insights: true,
            run_stitch: true,
            run_verify: true,
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
        // 1) Clotho — tag (and grow the dictionary on misses). Cadence-gated by
        // the daemon via cfg.run_clotho.
        let grow = if cfg.run_clotho {
            let clotho = Clotho::new(self.tooling);
            clotho.seed_dictionary().await?;
            let mems = self
                .tooling
                .list_user_memories(user, cfg.corpus_limit)
                .await?;
            clotho.grow_pass(&mems, cfg.grow_threshold).await?
        } else {
            GrowStats::default()
        };

        // 2) The insight stage — Lachesis routes & gates INSIDE Atropos::curate,
        // so the two share one cadence gate (they decouple once insights persist
        // to memory and Atropos can curate previously-routed threads).
        let insights = if cfg.run_insights {
            let orch = &self.tooling.config.moira.orchestrator;
            let candidates = self.tooling.list_categories(orch.candidate_cap).await?;
            let universe = self
                .tooling
                .total_memory_count(orch.universe_cap)
                .await?
                .max(1);
            let seeds: Vec<(String, String)> =
                candidates.iter().take(cfg.max_seeds).cloned().collect();
            let atropos = Atropos::new(self.tooling);
            atropos
                .curate(&seeds, &candidates, universe, cfg.max_hops)
                .await?
        } else {
            Vec::new()
        };

        // 3) #83: retroactive causal stitching — connect OLD memories whose
        // causal relation only became visible after both existed. Bounded and
        // hypothesis-grade (edges tagged lachesis-stitch).
        let stitch = if cfg.run_stitch {
            crate::agents::lachesis::stitch::Stitcher::new(self.tooling)
                .stitch_pass(user)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!("stitch stage failed (non-fatal): {e}");
                    Default::default()
                })
        } else {
            Default::default()
        };

        // 4) #91: the verification duty — aging hypotheses get an adversarial
        // review: promote (relabel VERIFIED) or retire (SUPERSEDE by a note,
        // which auto-demotes it in search since #92). Bounded and non-fatal.
        let verify = if cfg.run_verify {
            crate::agents::atropos::verify::Verifier::new(self.tooling)
                .verify_pass()
                .await
        } else {
            Default::default()
        };

        // Close the hive loop: curated hypotheses become first-class memories
        // (user `helixir`, SUPPORTS provenance) so any agent can recall them.
        let persisted = if insights.is_empty() {
            0
        } else {
            Atropos::new(self.tooling).persist_insights(&insights).await
        };

        info!(
            "orchestrator.full_pass(user={user}): clotho={} insights_stage={} — tagged {} (minted {}), {} insights ({} newly persisted to memory)",
            cfg.run_clotho,
            cfg.run_insights,
            grow.tagged_by_match + grow.reused_mint,
            grow.minted,
            insights.len(),
            persisted
        );
        Ok(PipelineRun {
            grow,
            insights,
            persisted,
            verify,
            stitch,
        })
    }
}
