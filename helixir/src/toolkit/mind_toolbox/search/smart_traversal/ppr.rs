//! Personalized PageRank over the ego-network collected during batched graph
//! expansion (elder-brain #9).
//!
//! The legacy scoring multiplies an edge weight (< 1) per hop, so relevance
//! decays geometrically with distance and far nodes can never surface — the
//! opposite of what long-range deduction needs. PPR replaces "farther = worse"
//! with "relevance mass flows along edges and accumulates": a node reached by
//! several coherent paths from the seeds outranks a weakly-attached close
//! neighbour. (Same move as HippoRAG, but over typed reasoning edges.)
//!
//! Scope note: the walk runs over the **ego-network** fetched by
//! `getConnectionsLevelBatch` (seeds + their ≤depth-hop neighbourhood), not
//! the whole graph. That keeps the read path O(depth) DB calls; the
//! approximation is exact for everything the search can return anyway.

use std::collections::HashMap;

/// A typed, already direction-weighted edge of the collected ego-network.
#[derive(Debug, Clone)]
pub struct PprEdge {
    pub from: String,
    pub to: String,
    pub weight: f64,
}

// Restart probability `alpha` (1 - alpha of the mass teleports back to seeds
// each step) and the iteration count are passed in from config.retrieval.ppr.
const CONVERGENCE_EPS: f64 = 1e-9;

/// Runs PPR and returns scores normalized to `[0, 1]` (max node = 1).
///
/// `personalization` — seed memory_id → non-negative weight (typically the
/// seed's phase-1 combined score). Unknown nodes in `edges` are added to the
/// graph automatically.
pub fn personalized_pagerank(
    edges: &[PprEdge],
    personalization: &HashMap<String, f64>,
    alpha: f64,
    iterations: usize,
) -> HashMap<String, f64> {
    // Index node ids (all &str borrow from `edges` / `personalization`).
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut ids: Vec<&str> = Vec::new();

    for e in edges {
        for id in [e.from.as_str(), e.to.as_str()] {
            if !index.contains_key(id) {
                index.insert(id, ids.len());
                ids.push(id);
            }
        }
    }
    for id in personalization.keys() {
        if !index.contains_key(id.as_str()) {
            index.insert(id.as_str(), ids.len());
            ids.push(id.as_str());
        }
    }

    let n = ids.len();
    if n == 0 {
        return HashMap::new();
    }

    // Out-weighted adjacency, row-normalized.
    let mut out_edges: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for e in edges {
        if e.weight <= 0.0 {
            continue;
        }
        let (f, t) = (index[e.from.as_str()], index[e.to.as_str()]);
        out_edges[f].push((t, e.weight));
    }
    for row in &mut out_edges {
        let sum: f64 = row.iter().map(|(_, w)| w).sum();
        if sum > 0.0 {
            for (_, w) in row.iter_mut() {
                *w /= sum;
            }
        }
    }

    // Normalized restart vector.
    let mut restart = vec![0.0f64; n];
    let p_sum: f64 = personalization.values().filter(|v| **v > 0.0).sum();
    if p_sum <= 0.0 {
        return HashMap::new();
    }
    for (id, w) in personalization {
        if *w > 0.0 {
            restart[index[id.as_str()]] = w / p_sum;
        }
    }

    // Power iteration. Dangling mass (nodes without out-edges) teleports home.
    let mut p = restart.clone();
    for _ in 0..iterations {
        let mut next = vec![0.0f64; n];
        let mut dangling = 0.0f64;
        for (i, mass) in p.iter().enumerate() {
            if *mass == 0.0 {
                continue;
            }
            if out_edges[i].is_empty() {
                dangling += mass;
                continue;
            }
            for (j, w) in &out_edges[i] {
                next[*j] += mass * w;
            }
        }
        let mut delta = 0.0f64;
        for i in 0..n {
            let value = (1.0 - alpha) * restart[i] + alpha * (next[i] + dangling * restart[i]);
            delta += (value - p[i]).abs();
            p[i] = value;
        }
        if delta < CONVERGENCE_EPS {
            break;
        }
    }

    let max = p.iter().cloned().fold(0.0f64, f64::max);
    if max <= 0.0 {
        return HashMap::new();
    }
    ids.iter()
        .enumerate()
        .map(|(i, id)| ((*id).to_string(), p[i] / max))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(from: &str, to: &str, weight: f64) -> PprEdge {
        PprEdge {
            from: from.into(),
            to: to.into(),
            weight,
        }
    }

    #[test]
    fn seed_gets_top_score() {
        let edges = vec![edge("s", "a", 1.0), edge("a", "b", 1.0)];
        let scores =
            personalized_pagerank(&edges, &HashMap::from([("s".to_string(), 1.0)]), 0.6, 20);
        assert!((scores["s"] - 1.0).abs() < 1e-9, "seed must be the maximum");
        assert!(scores["a"] > scores["b"], "mass decays along a single path");
    }

    #[test]
    fn convergent_paths_accumulate() {
        // d is fed by two seeds through distinct paths; e hangs off one seed.
        let edges = vec![
            edge("s1", "a", 1.0),
            edge("a", "d", 1.0),
            edge("s2", "b", 1.0),
            edge("b", "d", 1.0),
            edge("s1", "e", 1.0),
        ];
        let p = HashMap::from([("s1".to_string(), 1.0), ("s2".to_string(), 1.0)]);
        let scores = personalized_pagerank(&edges, &p, 0.6, 20);
        assert!(
            scores["d"] > scores["e"],
            "a node on two coherent paths must outrank a single-path one: {scores:?}"
        );
    }

    #[test]
    fn long_coherent_path_stays_reachable() {
        // 5-hop chain (the "Rajasthan → shale stocks" shape): the far end must
        // retain non-vanishing mass instead of being cut to ~0.
        let edges = vec![
            edge("s", "n1", 1.0),
            edge("n1", "n2", 1.0),
            edge("n2", "n3", 1.0),
            edge("n3", "n4", 1.0),
            edge("n4", "n5", 1.0),
        ];
        let scores =
            personalized_pagerank(&edges, &HashMap::from([("s".to_string(), 1.0)]), 0.6, 20);
        assert!(
            scores["n5"] > 0.01,
            "far end of a coherent chain must stay visible: {scores:?}"
        );
    }

    #[test]
    fn empty_inputs_are_safe() {
        assert!(personalized_pagerank(&[], &HashMap::new(), 0.6, 20).is_empty());
        let scores =
            personalized_pagerank(&[], &HashMap::from([("only".to_string(), 1.0)]), 0.6, 20);
        assert!((scores["only"] - 1.0).abs() < 1e-9);
    }
}
