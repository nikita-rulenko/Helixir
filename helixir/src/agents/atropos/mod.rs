//! Atropos — the Cutter (#48 / Moira). Curation → insight journal.
//!
//! Clotho weaves subsets; Lachesis routes and gates candidate threads; Atropos
//! curates the survivors into a journal worth a human's attention. It dedups
//! (a long thread subsumes its sub-threads), ranks by value (length × the
//! weakest link), enforces a quality bar, and emits first-class `Insight`s —
//! each a hypothesis carrying its provenance (the category path + the anchor
//! memories that witness it) and a lifecycle (`proposed → verified → refuted`).
//! It proposes; it never asserts (the charter, extended to generated insight).
//!
//! v0 routes via Lachesis per seed and curates the results; the insight journal
//! is JSONL written by the CLL (deploy-free). Persisting `Insight` nodes to the
//! graph and ranking by novelty-vs-journal-history are later steps.

#[cfg(feature = "nli")]
pub mod merge;
pub mod reconcile;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tracing::{info, warn};

use crate::agents::lachesis::{Lachesis, SubsetHypothesis};
use crate::toolkit::tooling_manager::ToolingManager;
use crate::toolkit::tooling_manager::types::ToolingError;

/// A memory that witnesses one link of an insight's chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightWitness {
    pub link: String,
    pub memory_id: String,
    pub snippet: String,
}

/// A curated cross-domain hypothesis — the journal's unit. Provenance-carrying,
/// always `requires_verification`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Insight {
    pub category_path: Vec<String>,
    pub hops: usize,
    /// Weakest PMI link — the chain's coherence floor.
    pub min_pmi: f64,
    /// Ranking score: `hops × min_pmi` (long AND coherent wins).
    pub value: f64,
    pub witnesses: Vec<InsightWitness>,
    /// Lifecycle: `proposed → verified → refuted`.
    pub status: String,
    pub requires_verification: bool,
}

/// Atropos the Cutter. Borrows the toolkit; composes Lachesis (the pipeline).
pub struct Atropos<'a> {
    tooling: &'a ToolingManager,
}

impl<'a> Atropos<'a> {
    pub fn new(tooling: &'a ToolingManager) -> Self {
        Self { tooling }
    }

    /// Route a thread from each seed category, then curate the survivors into
    /// ranked, deduped insights. `candidates` is the full category set routing
    /// considers; `universe` is the PMI N.
    pub async fn curate(
        &self,
        seeds: &[(String, String)],
        candidates: &[(String, String)],
        universe: usize,
        max_hops: usize,
    ) -> Result<Vec<Insight>, ToolingError> {
        let lachesis = Lachesis::new(self.tooling);
        let mut hyps: Vec<SubsetHypothesis> = Vec::new();
        for (sid, _) in seeds {
            if let Some(h) = lachesis
                .route_subsets(sid, candidates, universe, max_hops)
                .await?
            {
                hyps.push(h);
            }
        }
        let a = &self.tooling.config.moira.atropos;
        let insights = curate_hypotheses(hyps, a.quality_pmi_bar, a.min_hops);
        info!(
            "atropos.curate: {} insights from {} seeds",
            insights.len(),
            seeds.len()
        );
        Ok(insights)
    }
}

/// The pure curation policy: quality bar → build → dedup (a thread subsumes any
/// sub-thread of it) → rank by value. No DB, so it's unit-testable in isolation.
pub fn curate_hypotheses(
    hyps: Vec<SubsetHypothesis>,
    quality_pmi_bar: f64,
    min_hops: usize,
) -> Vec<Insight> {
    let mut insights: Vec<Insight> = hyps
        .iter()
        .filter(|h| h.hops >= min_hops && h.min_pmi >= quality_pmi_bar)
        .map(build_insight)
        .collect();

    // Strongest first, then keep a thread only if it isn't a subset of one
    // already kept — a 5-hop thread subsumes its 3-hop sub-threads.
    insights.sort_by(|a, b| {
        b.value
            .partial_cmp(&a.value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut kept: Vec<Insight> = Vec::new();
    for ins in insights {
        let cs: HashSet<&String> = ins.category_path.iter().collect();
        let subsumed = kept.iter().any(|k| {
            let ks: HashSet<&String> = k.category_path.iter().collect();
            cs.is_subset(&ks)
        });
        if !subsumed {
            kept.push(ins);
        }
    }
    kept
}

fn build_insight(h: &SubsetHypothesis) -> Insight {
    let category_path: Vec<String> = h.steps.iter().map(|s| s.category_name.clone()).collect();
    let mut witnesses = Vec::new();
    for (i, s) in h.steps.iter().enumerate() {
        if i == 0 {
            continue;
        }
        let link = format!("{} → {}", h.steps[i - 1].category_name, s.category_name);
        for w in &s.witnesses {
            witnesses.push(InsightWitness {
                link: link.clone(),
                memory_id: w.memory_id.clone(),
                snippet: w.snippet.clone(),
            });
        }
    }
    Insight {
        value: h.hops as f64 * h.min_pmi,
        category_path,
        hops: h.hops,
        min_pmi: h.min_pmi,
        witnesses,
        status: "proposed".to_string(),
        requires_verification: true,
    }
}

impl Atropos<'_> {
    /// Persist curated insights into the shared graph as FIRST-CLASS memories
    /// under the `helixir` user — hypothesis-framed (never asserted truth),
    /// provenance-linked to their witness memories with SUPPORTS edges, and
    /// recallable by ANY agent via collective search. The file journal remains
    /// the operator-facing log; this is what closes the hive loop: generated
    /// knowledge flows back into the memory it came from.
    ///
    /// Idempotent per category path: the hypothesis text is deterministic
    /// (volatile numbers stay out of the content), so a re-generated insight
    /// hits the same content_key group and is skipped.
    pub async fn persist_insights(&self, insights: &[Insight]) -> usize {
        use crate::llm::extractor::ExtractedMemory;
        use crate::toolkit::mind_toolbox::reasoning::ReasoningType;
        use crate::toolkit::tooling_manager::content_key::compute_content_key;

        let mut persisted = 0usize;
        for ins in insights {
            if ins.category_path.len() < 2 {
                continue;
            }
            let first = &ins.category_path[0];
            let last = ins.category_path.last().expect("len checked");
            let chain = ins.category_path.join(" -> ");
            // Deterministic content: path only — PMI/value change as the corpus
            // grows and would defeat content_key idempotency; they live in the
            // journal and in the edge strengths.
            let text = format!(
                concat!(
                    "HYPOTHESIS (generated, requires verification): an indirect ",
                    "cross-domain link may connect {first} to {last} via the chain ",
                    "{chain}. Found by Lachesis routing over shared-category subsets ",
                    "and curated by Atropos; this is a lead with provenance, NOT an ",
                    "asserted fact — if it holds, changes in {first} could propagate ",
                    "to {last}."
                ),
                first = first,
                last = last,
                chain = chain
            );

            // Idempotency: an identical hypothesis already lives in the graph.
            let key = compute_content_key(&text, "opinion");
            if !self.tooling.memories_in_group(&key).await.is_empty() {
                continue;
            }

            let vector = match self.tooling.embedder.generate(&text, true).await {
                Ok(v) => v,
                Err(e) => {
                    warn!("insight persist: embedding failed, skipping: {e}");
                    continue;
                }
            };
            let memory = ExtractedMemory {
                text,
                // `opinion` marks it epistemically subjective — the ontology's
                // closest type to "hypothesis"; certainty stays low by design.
                memory_type: "opinion".to_string(),
                certainty: 40,
                importance: (50 + (ins.hops as i32) * 5).min(85),
                entities: vec![],
                context: None,
            };
            let insight_id = match self
                .tooling
                .store_new_memory(&memory, "helixir", &vector, "moira-insight")
                .await
            {
                Ok((id, _)) => id,
                Err(e) => {
                    warn!("insight persist: store failed: {e}");
                    continue;
                }
            };

            // Provenance: every witness memory SUPPORTS the hypothesis.
            for w in &ins.witnesses {
                if w.memory_id.is_empty() || w.memory_id == insight_id {
                    continue;
                }
                if let Err(e) = self
                    .tooling
                    .reasoning_engine
                    .add_relation(&w.memory_id, &insight_id, ReasoningType::Supports, 60, None)
                    .await
                {
                    warn!(
                        "insight persist: witness edge {} -> {} failed: {e}",
                        w.memory_id, insight_id
                    );
                }
            }
            info!(
                "insight persisted as {insight_id} ({} witnesses): {}",
                ins.witnesses.len(),
                ins.category_path.join(" -> ")
            );
            persisted += 1;
        }
        persisted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::lachesis::{SubsetStep, SubsetWitness};

    fn step(name: &str, pmi: f64) -> SubsetStep {
        SubsetStep {
            category_id: format!("id_{name}"),
            category_name: name.to_string(),
            pmi_from_prev: pmi,
            witnesses: vec![SubsetWitness {
                memory_id: format!("mem_{name}"),
                snippet: format!("about {name}"),
            }],
        }
    }

    fn hyp(names_pmis: &[(&str, f64)]) -> SubsetHypothesis {
        let steps: Vec<SubsetStep> = names_pmis.iter().map(|(n, p)| step(n, *p)).collect();
        let min_pmi = names_pmis
            .iter()
            .skip(1)
            .map(|(_, p)| *p)
            .fold(f64::INFINITY, f64::min);
        SubsetHypothesis {
            hops: steps.len() - 1,
            min_pmi,
            requires_verification: true,
            steps,
        }
    }

    #[test]
    fn quality_bar_drops_weak_and_short() {
        // Below the PMI bar, and a single-hop link.
        let weak = hyp(&[("a", 0.0), ("b", 0.5)]);
        let short = hyp(&[("a", 0.0)]);
        assert!(curate_hypotheses(vec![weak, short], 1.0, 2).is_empty());
    }

    #[test]
    fn ranks_by_value_and_carries_provenance() {
        let small = hyp(&[("a", 0.0), ("b", 1.2), ("c", 1.2)]); // 2 hops × 1.2 = 2.4
        let big = hyp(&[("x", 0.0), ("y", 2.0), ("z", 2.0)]); // 2 hops × 2.0 = 4.0
        let out = curate_hypotheses(vec![small, big], 1.0, 2);
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0].category_path,
            vec!["x", "y", "z"],
            "highest value first"
        );
        assert!(out[0].requires_verification, "an insight is a hypothesis");
        assert_eq!(out[0].status, "proposed");
        assert!(!out[0].witnesses.is_empty(), "insights carry provenance");
    }

    #[test]
    fn dedups_subthreads_into_the_longest() {
        let full = hyp(&[("a", 0.0), ("b", 2.0), ("c", 2.0)]); // value 4.0
        let sub = hyp(&[("a", 0.0), ("b", 1.5)]); // {a,b} ⊂ {a,b,c} → dropped
        let out = curate_hypotheses(vec![sub, full], 1.0, 2);
        assert_eq!(out.len(), 1, "the sub-thread is subsumed");
        assert_eq!(out[0].hops, 2);
    }
}
