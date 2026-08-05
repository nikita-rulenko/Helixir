//! Graph routing and epistemic gating.

use super::*;

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
        let mut scratch = SubsetDfsScratch {
            on_path: HashSet::from([seed_category_id.to_string()]),
            cur: vec![(seed_category_id.to_string(), 0.0)],
            best: Vec::new(),
            best_key: (0usize, f64::INFINITY),
            budget: lc.dfs_budget as u64,
        };
        subset_dfs(seed_category_id, &adj, f64::INFINITY, &mut scratch);
        let mut best = scratch.best;
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
