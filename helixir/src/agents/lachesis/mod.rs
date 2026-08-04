//! Lachesis — the Measurer (#39 / Moira). The apophenia gate.
//!
//! Clotho weaves subsets; Lachesis routes chains *within* them and — the hard
//! part — decides which chains are MEANINGFUL versus coincidental. Two memories
//! sharing a tag is not a chain; without a gate Lachesis would emit thousands of
//! plausible-but-vacuous links (a confident bullshit generator). This module is
//! that gate: it scores a candidate chain and labels it a **hypothesis** or
//! **likely apophenia** — and a hypothesis is always flagged "requires
//! verification", never asserted as truth (the charter extended from stored
//! facts to generated connections — the moat).
//!
//! The score has two parts, both cheap and using what #33 already built:
//! - **coherence** = the *geometric mean* of the chain's edge weights (now real
//!   per-edge LLM strength × family weight). The geometric mean is length-fair:
//!   it measures per-hop quality, so a long coherent chain isn't punished for
//!   being long the way a raw weight product would be.
//! - **reasoning support** = the fraction of hops carried by a typed reasoning
//!   edge (IMPLIES/BECAUSE/SUPPORTS/CONTRADICTS/MEMORY_RELATION) rather than a
//!   bare associative bridge (`VIA_CATEGORY`). A chain held together only by
//!   shared tags is exactly the apophenia case the doc warns about.
//!
//! Later increments fold in category specificity (a thick axis like raw-material
//! is a weak bridge) and an LLM coherence judge for the borderline survivors.

pub mod stitch;

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::toolkit::mind_toolbox::search::smart_traversal::ConnectionPath;
use crate::toolkit::tooling_manager::ToolingManager;
use crate::toolkit::tooling_manager::types::ToolingError;

// The coherence bar, min-reasoning-support, and subset-PMI bar now live in
// config.moira.lachesis (coherence_bar / min_reasoning_support / subset_pmi_bar).

/// One hop of a candidate chain — the edge family and its weight.
pub struct ChainEdge<'a> {
    pub edge_type: &'a str,
    pub weight: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EpistemicLabel {
    /// Survived the gate — a connection worth surfacing, but unverified.
    PlausibleHypothesis,
    /// Failed the gate — weak per-hop coherence or bare association.
    LikelyApophenia,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoherenceVerdict {
    /// Geometric mean of the chain's edge weights — per-hop coherence in `0..1`.
    pub coherence: f64,
    /// Fraction of hops backed by a typed reasoning edge (vs `VIA_CATEGORY`).
    pub reasoning_support: f64,
    pub label: EpistemicLabel,
    /// Always `true` for a hypothesis: Lachesis proposes, it never adjudicates.
    pub requires_verification: bool,
    pub reason: String,
}

/// Is this edge family a typed reasoning relation (vs a bare associative
/// bridge)? Tolerates the `_IN` dual suffix used when an edge is walked against
/// its stored direction.
fn is_reasoning(edge_type: &str) -> bool {
    let base = edge_type.trim_end_matches("_IN");
    matches!(
        base,
        "IMPLIES" | "BECAUSE" | "SUPPORTS" | "CONTRADICTS" | "MEMORY_RELATION"
    )
}

/// The apophenia gate: score a candidate chain and label it. Pure — no DB — so
/// the policy is unit-testable in isolation. An empty chain is rejected.
pub fn assess(
    edges: &[ChainEdge],
    coherence_bar: f64,
    min_reasoning_support: f64,
) -> CoherenceVerdict {
    if edges.is_empty() {
        return CoherenceVerdict {
            coherence: 0.0,
            reasoning_support: 0.0,
            label: EpistemicLabel::LikelyApophenia,
            requires_verification: false,
            reason: "no hops — not a chain".to_string(),
        };
    }

    let n = edges.len() as f64;
    // Geometric mean via mean-of-logs (length-fair per-hop coherence). Clamp
    // weights off zero so a single 0-weight hop doesn't collapse the log.
    let log_mean: f64 = edges
        .iter()
        .map(|e| e.weight.clamp(1e-9, 1.0).ln())
        .sum::<f64>()
        / n;
    let coherence = log_mean.exp();

    let reasoning_hops = edges.iter().filter(|e| is_reasoning(e.edge_type)).count() as f64;
    let reasoning_support = reasoning_hops / n;

    let passes = coherence >= coherence_bar && reasoning_support >= min_reasoning_support;
    let (label, reason) = if passes {
        (
            EpistemicLabel::PlausibleHypothesis,
            format!(
                "per-hop coherence {coherence:.2} ≥ {coherence_bar:.2} and {:.0}% reasoning-backed \
                 — a plausible connection, requires verification",
                reasoning_support * 100.0
            ),
        )
    } else if reasoning_support < min_reasoning_support {
        (
            EpistemicLabel::LikelyApophenia,
            format!(
                "only {:.0}% of hops are reasoning-backed — mostly bare association",
                reasoning_support * 100.0
            ),
        )
    } else {
        (
            EpistemicLabel::LikelyApophenia,
            format!("per-hop coherence {coherence:.2} below the {coherence_bar:.2} bar"),
        )
    };

    CoherenceVerdict {
        coherence,
        reasoning_support,
        requires_verification: matches!(label, EpistemicLabel::PlausibleHypothesis),
        label,
        reason,
    }
}

mod router;
mod subsets;

pub use router::{GatedHypothesis, Lachesis};
use subsets::{SubsetDfsScratch, communities, polysemous_bridge, subset_dfs};
pub use subsets::{SubsetHypothesis, SubsetStep, SubsetWitness, pmi};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "mod_polysemy_tests.rs"]
mod polysemy_tests;
