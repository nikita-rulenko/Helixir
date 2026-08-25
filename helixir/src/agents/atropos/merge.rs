//! Contradiction-safe paraphrase consolidation (#43/#55/#168).
//!
//! Candidate discovery uses a private bounded vector path; it must never call
//! the public recall pipeline. NLI-confirmed equivalences are first assembled
//! into deterministic connected components and only then written, preventing
//! stale-key and traversal-order bugs in transitive merges.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use tracing::{debug, info};

use crate::llm::nli::NliJudge;
use crate::toolkit::tooling_manager::paraphrase::ParaphrasePair;

use super::Atropos;

#[derive(Debug, Default)]
pub struct MergeSummary {
    pub scanned: usize,
    pub candidates: usize,
    pub merged_groups: usize,
    pub nodes_restamped: usize,
    pub contradictions_blocked: usize,
}

impl Atropos<'_> {
    /// Scan recent memories for semantic duplicates and unify their fingerprint
    /// groups. Embeddings are only a candidate filter; the local NLI model is
    /// the final contradiction-safe judge.
    pub async fn merge_paraphrases(
        &self,
        limit: i64,
        cosine_threshold: f64,
    ) -> Result<MergeSummary> {
        let mut judge = NliJudge::load(&NliJudge::default_dir()).context(
            "NLI model unavailable — run `helixir model download`; the judge is required in every memory mode",
        )?;
        let briefs = self
            .tooling
            .list_recent_briefs(limit)
            .await
            .context("merge: list recent memory briefs")?;
        let pairs = self
            .tooling
            .paraphrase_pairs(&briefs, cosine_threshold, 8)
            .await
            .context("merge: bounded vector candidate discovery")?;
        let mut summary = MergeSummary {
            scanned: briefs.len(),
            candidates: pairs.len(),
            ..MergeSummary::default()
        };

        let mut confirmed = Vec::new();
        for pair in pairs {
            let verdict = judge
                .pair_verdict(&pair.seed_content, &pair.candidate_content)
                .with_context(|| {
                    format!(
                        "merge: NLI verdict for {} and {}",
                        pair.seed_id, pair.candidate_id
                    )
                })?;
            if verdict.same_fact {
                debug!(
                    "merge: NLI-confirmed pair {} ↔ {} (cosine {:.4})",
                    pair.seed_id, pair.candidate_id, pair.cosine
                );
                confirmed.push(pair);
            } else if verdict.contradiction {
                summary.contradictions_blocked += 1;
                debug!(
                    "merge: NLI blocked contradiction {} ↔ {}",
                    pair.seed_id, pair.candidate_id
                );
            }
        }

        for component in equivalence_components(&confirmed) {
            let before = summary.nodes_restamped;
            for noncanonical in component.keys.iter().skip(1) {
                let restamped = self
                    .tooling
                    .restamp_content_key_group(noncanonical, &component.canonical)
                    .await
                    .with_context(|| {
                        format!(
                            "merge: restamp content-key group {} in domain {}",
                            noncanonical, component.security_domain
                        )
                    })?;
                summary.merged_groups += 1;
                summary.nodes_restamped += restamped;
            }
            info!(
                "merge: unified {} fingerprint groups in {} onto {} ({} nodes updated)",
                component.keys.len(),
                component.security_domain,
                component.canonical,
                summary.nodes_restamped - before
            );
        }
        Ok(summary)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct MergeComponent {
    security_domain: String,
    canonical: String,
    keys: Vec<String>,
}

fn equivalence_components(pairs: &[ParaphrasePair]) -> Vec<MergeComponent> {
    let mut graphs: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    for pair in pairs {
        let graph = graphs.entry(pair.security_domain.clone()).or_default();
        graph
            .entry(pair.seed_content_key.clone())
            .or_default()
            .insert(pair.candidate_content_key.clone());
        graph
            .entry(pair.candidate_content_key.clone())
            .or_default()
            .insert(pair.seed_content_key.clone());
    }

    let mut components = Vec::new();
    for (security_domain, graph) in graphs {
        let mut visited = BTreeSet::new();
        for root in graph.keys() {
            if visited.contains(root) {
                continue;
            }
            let mut stack = vec![root.clone()];
            let mut keys = BTreeSet::new();
            while let Some(key) = stack.pop() {
                if !visited.insert(key.clone()) {
                    continue;
                }
                keys.insert(key.clone());
                if let Some(neighbours) = graph.get(&key) {
                    stack.extend(neighbours.iter().cloned());
                }
            }
            if keys.len() < 2 {
                continue;
            }
            let keys = keys.into_iter().collect::<Vec<_>>();
            components.push(MergeComponent {
                security_domain: security_domain.clone(),
                canonical: keys[0].clone(),
                keys,
            });
        }
    }
    components
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(domain: &str, left: &str, right: &str) -> ParaphrasePair {
        ParaphrasePair {
            seed_id: format!("mem_{left}"),
            seed_content: left.to_string(),
            seed_content_key: left.to_string(),
            candidate_id: format!("mem_{right}"),
            candidate_content: right.to_string(),
            candidate_content_key: right.to_string(),
            security_domain: domain.to_string(),
            cosine: 0.99,
        }
    }

    #[test]
    fn transitive_pairs_form_one_order_independent_component() {
        let forward = equivalence_components(&[
            pair("rbac:group:a", "b", "c"),
            pair("rbac:group:a", "a", "b"),
        ]);
        let reverse = equivalence_components(&[
            pair("rbac:group:a", "a", "b"),
            pair("rbac:group:a", "b", "c"),
        ]);
        assert_eq!(forward, reverse);
        assert_eq!(forward[0].canonical, "a");
        assert_eq!(forward[0].keys, ["a", "b", "c"]);
    }

    #[test]
    fn identical_keys_in_different_domains_never_join() {
        let components = equivalence_components(&[
            pair("rbac:group:a", "a", "b"),
            pair("rbac:group:b", "a", "c"),
        ]);
        assert_eq!(components.len(), 2);
        assert_ne!(components[0].security_domain, components[1].security_domain);
    }
}
