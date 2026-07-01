//! #43/#55 backstop — the contradiction-safe paraphrase merge.
//!
//! The exact-match fingerprint (`content_key`) groups identical facts. This pass
//! catches the rest: facts that MEAN the same but are worded differently, so they
//! carry different fingerprints and don't group. It finds high-cosine neighbours
//! (cheap, embeddings), then lets the local NLI judge make the safe final call —
//! unifying two fingerprint groups ONLY when the judge confirms "same fact" and
//! rules out contradiction. Opposites (dark vs light theme) are never merged.
//!
//! Each user keeps their own node; only the shared fingerprint changes. The pass
//! is idempotent and replay-safe (already-unified pairs are skipped).

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use crate::llm::nli::{NliJudge, NliLabel};

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
    /// Scan a user's memories for paraphrase duplicates and unify their
    /// fingerprints. `cosine_threshold` is the cheap embedding pre-filter; the
    /// NLI judge makes the contradiction-safe final decision.
    pub async fn merge_paraphrases(
        &self,
        limit: i64,
        cosine_threshold: f64,
    ) -> Result<MergeSummary> {
        let mut judge = NliJudge::load(&NliJudge::default_dir()).context(
            "NLI model unavailable — run `helixir model download` (collective/insights)",
        )?;

        let briefs = self.tooling.list_recent_briefs(limit).await;
        let mut summary = MergeSummary::default();
        let mut seen: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();

        for brief in &briefs {
            summary.scanned += 1;
            if brief.content.trim().is_empty() {
                continue;
            }
            // #68: never seed a merge pair from a raw_input node. A raw source
            // contains ALL its extracted atoms nearly verbatim, so atom↔raw
            // passes the entailment gate and fingerprint unification then chains
            // TRANSITIVELY (atomA ↔ raw ↔ atomB), conflating distinct facts into
            // one group — which the collective collapse folds into one row.
            if brief.memory_id.starts_with("raw_") {
                continue;
            }
            // Cheap pre-filter: cosine neighbours across the collective. A wide
            // temporal window so age never hides a paraphrase.
            let neighbours = match self
                .tooling
                .search_memory(
                    &brief.content,
                    "merge", // collective scope ignores the user_id
                    Some(8),
                    "contextual",
                    Some(36500.0),
                    None,
                    "collective",
                )
                .await
            {
                Ok(n) => n,
                Err(e) => {
                    warn!(
                        "merge: neighbour search failed for {}: {e}",
                        brief.memory_id
                    );
                    continue;
                }
            };

            for n in neighbours {
                if n.memory_id == brief.memory_id || n.score < cosine_threshold {
                    continue;
                }
                // #68: raw_input nodes are excluded from BOTH sides of a pair.
                if n.memory_id.starts_with("raw_") {
                    continue;
                }
                let pair = order_pair(&brief.memory_id, &n.memory_id);
                if !seen.insert(pair) {
                    continue;
                }

                // Fingerprints — skip if already grouped or unkeyed.
                let ck_a = if brief.content_key.is_empty() {
                    self.tooling.content_key_of(&brief.memory_id).await
                } else {
                    brief.content_key.clone()
                };
                let ck_b = self.tooling.content_key_of(&n.memory_id).await;
                if ck_a.is_empty() || ck_b.is_empty() || ck_a == ck_b {
                    continue;
                }
                summary.candidates += 1;

                // Contradiction-safe judgment (both directions inside is_same_fact).
                if judge
                    .is_same_fact(&brief.content, &n.content)
                    .unwrap_or(false)
                {
                    let canonical = ck_a.clone().min(ck_b.clone());
                    match self
                        .tooling
                        .unify_content_keys(&ck_a, &ck_b, &canonical)
                        .await
                    {
                        Ok(restamped) => {
                            summary.merged_groups += 1;
                            summary.nodes_restamped += restamped;
                            info!(
                                "merge: NLI-confirmed paraphrase → unified {} node(s) onto one fingerprint",
                                restamped
                            );
                        }
                        Err(e) => warn!("merge: unify failed: {e}"),
                    }
                } else if judge
                    .classify(&brief.content, &n.content)
                    .map(|(l, _)| l == NliLabel::Contradiction)
                    .unwrap_or(false)
                {
                    summary.contradictions_blocked += 1;
                    debug!(
                        "merge: NLI blocked a contradiction ({} ↔ {}) from merging",
                        brief.memory_id, n.memory_id
                    );
                }
            }
        }
        Ok(summary)
    }
}

fn order_pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}
