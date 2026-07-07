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

/// Pointwise mutual information of two subsets from their cardinalities — the
/// apophenia-safe overlap measure that routes the cross-domain (category) plane.
/// `> 0`: they co-occur MORE than chance (a real, surprising link); `0`: exactly
/// chance (no signal); `NEG_INFINITY`: never co-occur. A thick subset has a huge
/// cardinality in the denominator, so even large overlaps fall to ≈0 — it gates
/// itself out (the `raw material` problem solved by arithmetic). `total` is the
/// universe size N. One number = apophenia gate = surprise = specificity.
pub fn pmi(count_a: usize, count_b: usize, count_ab: usize, total: usize) -> f64 {
    if count_a == 0 || count_b == 0 || total == 0 {
        return 0.0;
    }
    if count_ab == 0 {
        return f64::NEG_INFINITY;
    }
    ((count_ab as f64 * total as f64) / (count_a as f64 * count_b as f64)).ln()
}

/// A memory that witnesses a chain hop — tagged with BOTH the categories whose
/// overlap forms the link. The provenance that makes a hypothesis verifiable.
#[derive(Debug, Clone, Serialize)]
pub struct SubsetWitness {
    pub memory_id: String,
    pub snippet: String,
}

/// One category in a routed cross-domain thread.
#[derive(Debug, Clone, Serialize)]
pub struct SubsetStep {
    pub category_id: String,
    pub category_name: String,
    /// PMI of the link from the previous step; `0.0` for the seed.
    pub pmi_from_prev: f64,
    /// Memories that witness the link from the previous step (its overlap
    /// members) — the anchors a reader checks to confirm or reject. Empty for
    /// the seed.
    pub witnesses: Vec<SubsetWitness>,
}

/// #91: label-propagation communities over the PMI adjacency. Deterministic
/// (sorted iteration, smallest-label ties) and cheap — the candidate set is
/// already capped. Two categories share a community when they sit in one
/// dense overlap neighbourhood; a chain hop that crosses communities through
/// a single pivot is the apophenia signature this exists to catch.
pub fn communities(
    adj: &std::collections::HashMap<String, Vec<(String, f64)>>,
) -> std::collections::HashMap<String, usize> {
    let mut nodes: Vec<&String> = adj.keys().collect();
    nodes.sort();
    let mut label: std::collections::HashMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| ((*n).clone(), i))
        .collect();
    for _ in 0..8 {
        let mut changed = false;
        for n in &nodes {
            let Some(neigh) = adj.get(*n) else { continue };
            if neigh.is_empty() {
                continue;
            }
            let mut counts: std::collections::HashMap<usize, usize> =
                std::collections::HashMap::new();
            for (m, _) in neigh {
                if let Some(l) = label.get(m) {
                    *counts.entry(*l).or_insert(0) += 1;
                }
            }
            let Some(best) = counts
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
                .map(|(l, _)| *l)
            else {
                continue;
            };
            if label.get(*n) != Some(&best) {
                label.insert((*n).clone(), best);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    label
}

/// #91: find the first POLYSEMOUS BRIDGE in a routed category chain — an
/// interior pivot whose neighbours (a) sit in different communities and
/// (b) are not adjacent themselves, i.e. the pivot is the only thing
/// holding two unrelated domains together ("benchmarking" bridging finance
/// and software). Returns the pivot's index. Measured first: embedding
/// cohesion/bimodality CANNOT catch this case — the embedder itself
/// conflates the senses — so the signal must be topological.
pub fn polysemous_bridge(
    path: &[(String, f64)],
    adj: &std::collections::HashMap<String, Vec<(String, f64)>>,
    comm: &std::collections::HashMap<String, usize>,
) -> Option<usize> {
    for i in 1..path.len().saturating_sub(1) {
        let (prev, next) = (&path[i - 1].0, &path[i + 1].0);
        let cross = match (comm.get(prev), comm.get(next)) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        };
        if !cross {
            continue;
        }
        let direct = adj
            .get(prev)
            .is_some_and(|ns| ns.iter().any(|(m, _)| m == next));
        if !direct {
            return Some(i);
        }
    }
    None
}

/// A cross-domain thread over the subset-overlap graph — the generative output:
/// "these distant domains connect through this chain of above-chance overlaps".
/// Always a hypothesis, never a verdict.
#[derive(Debug, Clone, Serialize)]
pub struct SubsetHypothesis {
    /// Ordered category chain, seed → … → end.
    pub steps: Vec<SubsetStep>,
    pub hops: usize,
    /// The weakest PMI link — a chain is only as coherent as its weakest hop.
    pub min_pmi: f64,
    /// Always `true`: Lachesis proposes the connection, it does not assert it.
    pub requires_verification: bool,
}

/// DFS for the longest simple path over the PMI subset graph, ranked by hops then
/// the weakest link (min PMI). `adj`: category_id → [(neighbour, pmi)].
fn subset_dfs(
    node: &str,
    adj: &std::collections::HashMap<String, Vec<(String, f64)>>,
    on_path: &mut HashSet<String>,
    cur: &mut Vec<(String, f64)>,
    cur_min: f64,
    best: &mut Vec<(String, f64)>,
    best_key: &mut (usize, f64),
    budget: &mut u64,
) {
    if *budget == 0 {
        return;
    }
    *budget -= 1;

    if cur.len() > best_key.0 || (cur.len() == best_key.0 && cur_min > best_key.1) {
        *best_key = (cur.len(), cur_min);
        *best = cur.clone();
    }

    if let Some(neighbours) = adj.get(node) {
        for (next, p) in neighbours {
            if on_path.contains(next) {
                continue;
            }
            on_path.insert(next.clone());
            cur.push((next.clone(), *p));
            subset_dfs(
                next,
                adj,
                on_path,
                cur,
                cur_min.min(*p),
                best,
                best_key,
                budget,
            );
            cur.pop();
            on_path.remove(next);
            if *budget == 0 {
                return;
            }
        }
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
        let lc = &self.tooling.config.moira.lachesis;
        let verdict = assess(&edges, lc.coherence_bar, lc.min_reasoning_support);
        Ok(Some(GatedHypothesis { path, verdict }))
    }

    /// PMI link strength between two category subsets over a `universe` of N
    /// memories — the apophenia-safe overlap Lachesis routes the cross-domain
    /// plane with. Fetches both member sets and intersects them in memory (the
    /// deploy-free v0; a `CO_OCCURS`-edge cache replaces the fetch at scale).
    pub async fn subset_pmi(
        &self,
        category_a_id: &str,
        category_b_id: &str,
        universe: usize,
    ) -> Result<f64, ToolingError> {
        let a = self.tooling.category_member_ids(category_a_id).await?;
        let b = self.tooling.category_member_ids(category_b_id).await?;
        let overlap = a.iter().filter(|id| b.contains(*id)).count();
        Ok(pmi(a.len(), b.len(), overlap, universe))
    }

    /// Route a cross-domain thread over the subset-overlap graph: from a seed
    /// category, walk to other `candidates` through above-chance (PMI ≥ bar)
    /// overlaps, and return the longest such chain. This is the generative move —
    /// "domain A connects to distant domain Z via this chain of overlaps" — but
    /// only over links that beat chance, so a thick axis (PMI ≈ 0) never carries
    /// the route. `candidates` are `(category_id, name)` to consider; `universe`
    /// is N. Returns `None` if the seed has no qualifying neighbour.
    ///
    /// v0 takes the candidate set explicitly (a test passes a few; production
    /// passes the dictionary or the topic-relevant categories) and computes PMI
    /// on the fly — a `CO_OCCURS`-edge cache replaces the fetch at scale.
    pub async fn route_subsets(
        &self,
        seed_category_id: &str,
        candidates: &[(String, String)],
        universe: usize,
        max_hops: usize,
    ) -> Result<Option<SubsetHypothesis>, ToolingError> {
        let lc = self.tooling.config.moira.lachesis.clone();
        // Unique candidate ids (+ names), seed included.
        let mut name_of: HashMap<String, String> = HashMap::new();
        for (id, name) in candidates {
            name_of.entry(id.clone()).or_insert_with(|| name.clone());
        }
        if !name_of.contains_key(seed_category_id) {
            return Ok(None);
        }

        // Member set per category (cached).
        let mut members: HashMap<String, HashSet<String>> = HashMap::new();
        for id in name_of.keys() {
            members.insert(id.clone(), self.tooling.category_member_ids(id).await?);
        }

        // Symmetric PMI adjacency over qualifying links.
        let ids: Vec<String> = name_of.keys().cloned().collect();
        let mut adj: HashMap<String, Vec<(String, f64)>> = HashMap::new();
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let (a, b) = (&ids[i], &ids[j]);
                let ma = &members[a];
                let mb = &members[b];
                let overlap = ma.iter().filter(|m| mb.contains(*m)).count();
                let p = pmi(ma.len(), mb.len(), overlap, universe);
                if p >= lc.subset_pmi_bar {
                    adj.entry(a.clone()).or_default().push((b.clone(), p));
                    adj.entry(b.clone()).or_default().push((a.clone(), p));
                }
            }
        }

        // Longest high-PMI simple path from the seed.
        let mut best: Vec<(String, f64)> = Vec::new();
        let mut best_key = (0usize, f64::INFINITY);
        let mut on_path: HashSet<String> = HashSet::new();
        on_path.insert(seed_category_id.to_string());
        let mut cur: Vec<(String, f64)> = vec![(seed_category_id.to_string(), 0.0)];
        let mut budget: u64 = lc.dfs_budget as u64;
        subset_dfs(
            seed_category_id,
            &adj,
            &mut on_path,
            &mut cur,
            f64::INFINITY,
            &mut best,
            &mut best_key,
            &mut budget,
        );
        // Respect max_hops by truncating an over-long thread.
        if best.len() > max_hops + 1 {
            best.truncate(max_hops + 1);
        }

        // #91: the polysemy guard. A pivot bridging two communities that
        // share no direct link is an apophenia hub (finance-benchmarking vs
        // software-benchmarking fused into one category) — keep the coherent
        // prefix up to the pivot, drop the cross-domain jump.
        if lc.polysemy_guard {
            let comm = communities(&adj);
            if let Some(pivot_idx) = polysemous_bridge(&best, &adj, &comm) {
                let pivot_name = name_of
                    .get(&best[pivot_idx].0)
                    .cloned()
                    .unwrap_or_else(|| best[pivot_idx].0.clone());
                tracing::info!(
                    "Polysemy guard (#91): '{pivot_name}' bridges two unrelated \
                     communities — thread truncated at the pivot"
                );
                best.truncate(pivot_idx + 1);
            }
        }

        if best.len() < 2 {
            return Ok(None);
        }

        let min_pmi = best
            .iter()
            .skip(1)
            .map(|(_, p)| *p)
            .fold(f64::INFINITY, f64::min);

        // Drill each hop down to its anchor memories — the overlap members that
        // witness the link. This is what makes a hypothesis verifiable: read the
        // anchors and the connection stands or falls.
        let mut steps: Vec<SubsetStep> = Vec::with_capacity(best.len());
        for (i, (id, p)) in best.iter().enumerate() {
            let mut witnesses = Vec::new();
            if i > 0 {
                let prev = &best[i - 1].0;
                if let (Some(ma), Some(mb)) = (members.get(prev), members.get(id)) {
                    let overlap: Vec<String> = ma
                        .iter()
                        .filter(|m| mb.contains(*m))
                        .take(lc.witnesses_per_hop)
                        .cloned()
                        .collect();
                    for mid in overlap {
                        let snippet = self
                            .tooling
                            .memory_content(&mid)
                            .await?
                            .map(|c| c.chars().take(lc.snippet_len).collect())
                            .unwrap_or_default();
                        witnesses.push(SubsetWitness {
                            memory_id: mid,
                            snippet,
                        });
                    }
                }
            }
            steps.push(SubsetStep {
                category_name: name_of.get(id).cloned().unwrap_or_default(),
                category_id: id.clone(),
                pmi_from_prev: *p,
                witnesses,
            });
        }
        let hops = steps.len() - 1;
        Ok(Some(SubsetHypothesis {
            hops,
            min_pmi,
            requires_verification: true,
            steps,
        }))
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
        let v = assess(&[e("IMPLIES", 0.72), e("BECAUSE", 0.70)], 0.5, 0.5);
        assert_eq!(v.label, EpistemicLabel::PlausibleHypothesis);
        assert!(
            v.requires_verification,
            "a hypothesis is never asserted as truth"
        );
        assert!(
            v.coherence > 0.7 && v.coherence <= 0.72,
            "geomean ~0.71, got {}",
            v.coherence
        );
        assert_eq!(v.reasoning_support, 1.0);
    }

    #[test]
    fn bare_association_chain_is_apophenia() {
        // Two memories linked only by shared tags — the canonical apophenia case.
        let v = assess(&[e("VIA_CATEGORY", 0.5), e("VIA_CATEGORY", 0.5)], 0.5, 0.5);
        assert_eq!(v.label, EpistemicLabel::LikelyApophenia);
        assert!(!v.requires_verification);
        assert_eq!(v.reasoning_support, 0.0);
    }

    #[test]
    fn weak_reasoning_chain_is_apophenia() {
        // Reasoning-typed but the per-hop confidence is too low to trust.
        let v = assess(&[e("MEMORY_RELATION", 0.30), e("IMPLIES", 0.35)], 0.5, 0.5);
        assert_eq!(v.label, EpistemicLabel::LikelyApophenia);
        assert!(v.coherence < 0.5);
    }

    #[test]
    fn geometric_mean_is_length_fair() {
        // A long, firmly-reasoned chain must not be rejected just for being long
        // (a raw weight product would underflow the bar).
        let long: Vec<ChainEdge> = (0..8).map(|_| e("IMPLIES", 0.7)).collect();
        let v = assess(&long, 0.5, 0.5);
        assert_eq!(v.label, EpistemicLabel::PlausibleHypothesis);
        assert!(
            (v.coherence - 0.7).abs() < 1e-9,
            "geomean of all-0.7 is 0.7, got {}",
            v.coherence
        );
    }

    #[test]
    fn mixed_chain_keeps_a_reasoning_majority() {
        // One associative bridge among reasoning hops still passes the support bar.
        let v = assess(
            &[e("IMPLIES", 0.7), e("VIA_CATEGORY", 0.6), e("BECAUSE", 0.7)],
            0.5,
            0.5,
        );
        assert!(v.reasoning_support >= 0.5);
        assert_eq!(v.label, EpistemicLabel::PlausibleHypothesis);
    }

    #[test]
    fn empty_is_not_a_chain() {
        assert_eq!(assess(&[], 0.5, 0.5).label, EpistemicLabel::LikelyApophenia);
    }

    // --- PMI subset-overlap routing (the cross-domain apophenia guard) ---

    #[test]
    fn pmi_thick_axis_gates_itself_out() {
        // A subset covering the whole universe co-occurs with anything at exactly
        // chance → PMI 0, regardless of overlap. The raw-material problem, solved.
        assert!(pmi(10, 100, 10, 100).abs() < 1e-9);
    }

    #[test]
    fn pmi_specific_pair_scores_high() {
        // Two small subsets fully overlapping, far above chance.
        assert!(pmi(5, 5, 5, 1000) > 3.0);
    }

    #[test]
    fn pmi_no_overlap_is_neg_inf() {
        assert_eq!(pmi(10, 10, 0, 1000), f64::NEG_INFINITY);
    }

    #[test]
    fn pmi_specific_beats_thick() {
        let specific = pmi(5, 5, 5, 100); // narrow, fully overlapping
        let thick = pmi(5, 100, 5, 100); // B spans the whole universe
        assert!(
            specific > thick,
            "specific {specific} should beat thick {thick}"
        );
    }
}

#[cfg(test)]
mod polysemy_tests {
    use super::*;
    use std::collections::HashMap;

    fn adj_of(edges: &[(&str, &str)]) -> HashMap<String, Vec<(String, f64)>> {
        let mut adj: HashMap<String, Vec<(String, f64)>> = HashMap::new();
        for (a, b) in edges {
            adj.entry((*a).into()).or_default().push(((*b).into(), 1.0));
            adj.entry((*b).into()).or_default().push(((*a).into(), 1.0));
        }
        adj
    }

    /// Two dense cliques joined ONLY through one pivot — the benchmarking
    /// shape. The guard must find the pivot; label propagation must put the
    /// cliques in different communities.
    #[test]
    fn pivot_between_two_cliques_is_a_polysemous_bridge() {
        let adj = adj_of(&[
            // finance clique
            ("energy", "commodity"),
            ("commodity", "markets"),
            ("energy", "markets"),
            // software clique
            ("debugging", "queries"),
            ("queries", "testing"),
            ("debugging", "testing"),
            // the two-faced pivot
            ("markets", "benchmarking"),
            ("benchmarking", "debugging"),
        ]);
        let comm = communities(&adj);
        assert_ne!(
            comm["markets"], comm["debugging"],
            "cliques must land in different communities: {comm:?}"
        );

        let path: Vec<(String, f64)> =
            ["energy", "markets", "benchmarking", "debugging", "queries"]
                .iter()
                .map(|s| (s.to_string(), 1.0))
                .collect();
        // The pivot's own label lands on one side, so the detected crossing
        // is the hop AT or NEXT TO the pivot (index 1 or 2) — either
        // truncation point drops the cross-domain jump.
        let bridge = polysemous_bridge(&path, &adj, &comm);
        assert!(
            matches!(bridge, Some(1) | Some(2)),
            "the crossing must be detected around the pivot: {bridge:?}"
        );
    }

    /// A chain inside ONE community never trips the guard, however long.
    #[test]
    fn intra_community_chain_is_untouched() {
        let adj = adj_of(&[
            ("retrieval", "knowledge"),
            ("knowledge", "databases"),
            ("databases", "version-control"),
            ("retrieval", "databases"),
            ("knowledge", "version-control"),
        ]);
        let comm = communities(&adj);
        let path: Vec<(String, f64)> = ["retrieval", "knowledge", "databases", "version-control"]
            .iter()
            .map(|s| (s.to_string(), 1.0))
            .collect();
        assert_eq!(polysemous_bridge(&path, &adj, &comm), None);
    }

    /// A cross-community hop whose endpoints ALSO share a direct link is a
    /// genuine bridge (two domains really touching), not polysemy — kept.
    #[test]
    fn genuine_cross_domain_link_is_kept() {
        let adj = adj_of(&[
            ("a1", "a2"),
            ("a1", "a3"),
            ("a2", "a3"),
            ("b1", "b2"),
            ("b1", "b3"),
            ("b2", "b3"),
            ("a3", "pivot"),
            ("pivot", "b1"),
            // the endpoints of the pivot hop know each other directly:
            ("a3", "b1"),
        ]);
        let comm = communities(&adj);
        let path: Vec<(String, f64)> = ["a1", "a3", "pivot", "b1", "b2"]
            .iter()
            .map(|s| (s.to_string(), 1.0))
            .collect();
        assert_eq!(
            polysemous_bridge(&path, &adj, &comm),
            None,
            "direct a3-b1 adjacency proves the domains genuinely touch"
        );
    }
}
