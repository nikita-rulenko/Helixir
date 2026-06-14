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

use serde::Serialize;

use crate::toolkit::mind_toolbox::search::smart_traversal_v2::ConnectionPath;
use crate::toolkit::tooling_manager::ToolingManager;
use crate::toolkit::tooling_manager::types::ToolingError;

/// Per-hop coherence a chain must clear (geometric mean of edge weights).
const COHERENCE_BAR: f64 = 0.5;
/// A chain must be at least half typed reasoning, not bare association.
const MIN_REASONING_SUPPORT: f64 = 0.5;

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
pub fn assess(edges: &[ChainEdge]) -> CoherenceVerdict {
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

    let passes = coherence >= COHERENCE_BAR && reasoning_support >= MIN_REASONING_SUPPORT;
    let (label, reason) = if passes {
        (
            EpistemicLabel::PlausibleHypothesis,
            format!(
                "per-hop coherence {coherence:.2} ≥ {COHERENCE_BAR:.2} and {:.0}% reasoning-backed \
                 — a plausible connection, requires verification",
                reasoning_support * 100.0
            ),
        )
    } else if reasoning_support < MIN_REASONING_SUPPORT {
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
            format!("per-hop coherence {coherence:.2} below the {COHERENCE_BAR:.2} bar"),
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

/// A routed chain plus the gate's verdict on it.
#[derive(Debug, Clone, Serialize)]
pub struct GatedHypothesis {
    pub path: ConnectionPath,
    pub verdict: CoherenceVerdict,
}

/// Lachesis the Measurer. Borrows the toolkit it routes over (mirrors Clotho).
pub struct Lachesis<'a> {
    tooling: &'a ToolingManager,
}

impl<'a> Lachesis<'a> {
    pub fn new(tooling: &'a ToolingManager) -> Self {
        Self { tooling }
    }

    /// Route a chain between two topics and gate it: find the connecting path
    /// (`connect_memories`), then assess its coherence. Returns the chain with
    /// its verdict, or `None` when no path connects the topics at all.
    pub async fn route(
        &self,
        topic_a: &str,
        topic_b: &str,
        user_id: &str,
        max_depth: usize,
    ) -> Result<Option<GatedHypothesis>, ToolingError> {
        let Some(path) = self
            .tooling
            .connect_memories(topic_a, topic_b, user_id, max_depth)
            .await?
        else {
            return Ok(None);
        };

        let edges: Vec<ChainEdge> = path
            .edges
            .iter()
            .map(|e| ChainEdge {
                edge_type: e.edge_type.as_str(),
                weight: e.weight,
            })
            .collect();
        let verdict = assess(&edges);
        Ok(Some(GatedHypothesis { path, verdict }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(t: &str, w: f64) -> ChainEdge<'_> {
        ChainEdge {
            edge_type: t,
            weight: w,
        }
    }

    #[test]
    fn reasoning_backed_chain_is_a_hypothesis() {
        let v = assess(&[e("IMPLIES", 0.72), e("BECAUSE", 0.70)]);
        assert_eq!(v.label, EpistemicLabel::PlausibleHypothesis);
        assert!(v.requires_verification, "a hypothesis is never asserted as truth");
        assert!(v.coherence > 0.7 && v.coherence <= 0.72, "geomean ~0.71, got {}", v.coherence);
        assert_eq!(v.reasoning_support, 1.0);
    }

    #[test]
    fn bare_association_chain_is_apophenia() {
        // Two memories linked only by shared tags — the canonical apophenia case.
        let v = assess(&[e("VIA_CATEGORY", 0.5), e("VIA_CATEGORY", 0.5)]);
        assert_eq!(v.label, EpistemicLabel::LikelyApophenia);
        assert!(!v.requires_verification);
        assert_eq!(v.reasoning_support, 0.0);
    }

    #[test]
    fn weak_reasoning_chain_is_apophenia() {
        // Reasoning-typed but the per-hop confidence is too low to trust.
        let v = assess(&[e("MEMORY_RELATION", 0.30), e("IMPLIES", 0.35)]);
        assert_eq!(v.label, EpistemicLabel::LikelyApophenia);
        assert!(v.coherence < COHERENCE_BAR);
    }

    #[test]
    fn geometric_mean_is_length_fair() {
        // A long, firmly-reasoned chain must not be rejected just for being long
        // (a raw weight product would underflow the bar).
        let long: Vec<ChainEdge> = (0..8).map(|_| e("IMPLIES", 0.7)).collect();
        let v = assess(&long);
        assert_eq!(v.label, EpistemicLabel::PlausibleHypothesis);
        assert!((v.coherence - 0.7).abs() < 1e-9, "geomean of all-0.7 is 0.7, got {}", v.coherence);
    }

    #[test]
    fn mixed_chain_keeps_a_reasoning_majority() {
        // One associative bridge among reasoning hops still passes the support bar.
        let v = assess(&[e("IMPLIES", 0.7), e("VIA_CATEGORY", 0.6), e("BECAUSE", 0.7)]);
        assert!(v.reasoning_support >= MIN_REASONING_SUPPORT);
        assert_eq!(v.label, EpistemicLabel::PlausibleHypothesis);
    }

    #[test]
    fn empty_is_not_a_chain() {
        assert_eq!(assess(&[]).label, EpistemicLabel::LikelyApophenia);
    }
}
