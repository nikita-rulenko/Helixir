//! Subset statistics and route candidates.

use super::*;

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

/// Mutable scratch threaded through [`subset_dfs`]: the walk state, the best
/// path found so far (ranked by hops, then weakest link) and the node budget.
pub(super) struct SubsetDfsScratch {
    pub(super) on_path: HashSet<String>,
    pub(super) cur: Vec<(String, f64)>,
    pub(super) best: Vec<(String, f64)>,
    pub(super) best_key: (usize, f64),
    pub(super) budget: u64,
}

/// DFS for the longest simple path over the PMI subset graph, ranked by hops then
/// the weakest link (min PMI). `adj`: category_id → [(neighbour, pmi)].
pub(super) fn subset_dfs(
    node: &str,
    adj: &std::collections::HashMap<String, Vec<(String, f64)>>,
    cur_min: f64,
    scratch: &mut SubsetDfsScratch,
) {
    if scratch.budget == 0 {
        return;
    }
    scratch.budget -= 1;

    if scratch.cur.len() > scratch.best_key.0
        || (scratch.cur.len() == scratch.best_key.0 && cur_min > scratch.best_key.1)
    {
        scratch.best_key = (scratch.cur.len(), cur_min);
        scratch.best = scratch.cur.clone();
    }

    if let Some(neighbours) = adj.get(node) {
        for (next, p) in neighbours {
            if scratch.on_path.contains(next) {
                continue;
            }
            scratch.on_path.insert(next.clone());
            scratch.cur.push((next.clone(), *p));
            subset_dfs(next, adj, cur_min.min(*p), scratch);
            scratch.cur.pop();
            scratch.on_path.remove(next);
            if scratch.budget == 0 {
                return;
            }
        }
    }
}
