//! Clotho — the Spinner (#33 / Moira).
//!
//! Roams memories and tags them from a controlled, growing vocabulary; shared
//! tags weave seemingly-unrelated memories into subsets (the substrate the
//! cross-domain bridge routes over, and the subsets Lachesis will later route
//! within). This module is the agent's POLICY — which dictionary, what
//! threshold, ancestor propagation, charter escalation. The category PRIMITIVES
//! it drives (`ensure_category`, `tag_memory`, `search_similar_categories`, …)
//! live in [`ToolingManager`]; Clotho only composes them.

mod dictionary;

use tracing::{info, warn};

use crate::llm::providers::base::LlmProvider;
use crate::toolkit::tooling_manager::ToolingManager;
use crate::toolkit::tooling_manager::types::{Clarification, ToolingError};

// Dominance margin (grow-pass: tag only categories within this much of the best
// match — keeps the top domain(s), drops the noise-floor smear that wove
// spurious cross-domain bridges) now lives in config.moira.clotho.dominance_margin.

/// One category an auto-tag pass attached to a memory.
#[derive(Debug, Clone)]
pub struct AutoTagHit {
    pub category_id: String,
    pub name: String,
    /// Vector similarity that earned the tag; `1.0` for a propagated ancestor
    /// (structural, not measured).
    pub score: f64,
}

/// Outcome of one embedding-match auto-tag pass over a single memory.
#[derive(Debug, Default)]
pub struct AutoTagOutcome {
    /// Categories tagged — matched leaves plus their propagated ancestors.
    pub tagged: Vec<AutoTagHit>,
    /// Set when NOTHING cleared the threshold: Clotho met an unknown and
    /// escalates per the charter instead of inventing a category silently.
    pub escalation: Option<Clarification>,
}

/// Outcome of a grow-and-tag pass over a corpus.
#[derive(Debug, Default)]
pub struct GrowStats {
    pub scanned: usize,
    /// Tagged from a category already in the dictionary.
    pub tagged_by_match: usize,
    /// New categories minted via the LLM (the dictionary grew).
    pub minted: usize,
    /// Tagged by a category minted earlier in THIS pass (reuse — convergence).
    pub reused_mint: usize,
    pub failed: usize,
}

/// Clotho the Spinner. Borrows the toolkit it drives; cheap to construct per
/// pass and holds no state of its own yet (the daemon loop will own cadence).
pub struct Clotho<'a> {
    tooling: &'a ToolingManager,
}

impl<'a> Clotho<'a> {
    pub fn new(tooling: &'a ToolingManager) -> Self {
        Self { tooling }
    }

    /// Seed the starter dictionary — idempotent on nodes (via `ensure_category`).
    /// Returns the count ensured. Hierarchy edges are (re)linked each run;
    /// harmless today since `SUBCATEGORY_OF` is not read at query time, but worth
    /// making idempotent before that changes.
    pub async fn seed_dictionary(&self) -> Result<usize, ToolingError> {
        // Pass 1: every node exists before any link references it as a parent.
        for (name, kind, desc, _) in dictionary::CATEGORY_SEEDS {
            self.tooling.ensure_category(name, kind, desc).await?;
        }
        // Pass 2: wire the hierarchy.
        for (name, _, _, parent) in dictionary::CATEGORY_SEEDS {
            if let Some(parent) = parent {
                let child_id = self.tooling.ensure_category(name, "", "").await?;
                let parent_id = self.tooling.ensure_category(parent, "", "").await?;
                self.tooling.link_subcategory(&child_id, &parent_id).await?;
            }
        }
        let n = dictionary::CATEGORY_SEEDS.len();
        info!("clotho.seed_dictionary: {n} categories ensured");
        Ok(n)
    }

    /// Clotho's core move: match `content` against the dictionary by vector
    /// similarity and tag every category that clears `threshold` — plus each
    /// match's ancestors (broader jump-planes). When nothing clears the bar,
    /// return a charter escalation instead of inventing a category. `top_k`
    /// caps the candidates. Idempotent-ish: re-tagging just re-asserts the edge.
    pub async fn auto_tag(
        &self,
        memory_id: &str,
        content: &str,
        top_k: i64,
        threshold: f64,
    ) -> Result<AutoTagOutcome, ToolingError> {
        let mut outcome = AutoTagOutcome::default();
        if content.trim().is_empty() {
            return Ok(outcome);
        }

        // SearchV exposes no readable similarity score (see helixdb-hql-gotchas),
        // so match by cosine in memory over the small dictionary. Category
        // vectors are embeddings of "name: description"; the embedder caches them
        // so repeated passes are cheap. (A batch daemon pass should embed the
        // dictionary once up front rather than per memory.)
        let content_vec = self.tooling.embed_text(content).await?;
        let mut matched: Vec<(String, f64)> = Vec::new();
        for (name, _kind, desc, _parent) in dictionary::CATEGORY_SEEDS {
            let cat_vec = self.tooling.embed_text(&format!("{name}: {desc}")).await?;
            let score = cosine(&content_vec, &cat_vec);
            if score >= threshold {
                matched.push((name.to_string(), score));
            }
        }
        // Strongest first, then cap to `top_k` leaves.
        matched.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        if top_k >= 0 {
            matched.truncate(top_k as usize);
        }

        if matched.is_empty() {
            outcome.escalation = Some(Clarification {
                conflict_type: "no_category_match".to_string(),
                new_content: content.chars().take(200).collect(),
                existing_memory_id: Some(memory_id.to_string()),
                existing_content: None,
                suggested_question: format!(
                    "No dictionary category fits this memory above the {threshold:.2} \
                     similarity bar. Create a new category for it, or leave it untagged?"
                ),
                decision_taken: "left untagged — escalated per charter (no silent category \
                                 invention)"
                    .to_string(),
                confidence: 0,
            });
            return Ok(outcome);
        }

        // Tag matched leaves, then propagate up the seed hierarchy. Ancestors are
        // structurally certain (score 1.0) — no DB round-trip to climb the tree.
        let mut plan: Vec<(String, f64)> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (name, score) in &matched {
            if seen.insert(name.clone()) {
                plan.push((name.clone(), *score));
            }
            for ancestor in dictionary::ancestors(name) {
                if seen.insert(ancestor.to_string()) {
                    plan.push((ancestor.to_string(), 1.0));
                }
            }
        }

        for (name, score) in plan {
            let Some(category_id) = self.tooling.get_category_id(&name).await? else {
                warn!(
                    "clotho.auto_tag: matched category '{name}' has no node (dictionary unseeded?)"
                );
                continue;
            };
            let confidence = (score.clamp(0.0, 1.0) * 100.0).round() as i64;
            if let Err(e) = self
                .tooling
                .tag_memory(memory_id, &category_id, confidence, "clotho-embed")
                .await
            {
                warn!("clotho.auto_tag: tag_memory failed for {memory_id} -> {name}: {e}");
                continue;
            }
            outcome.tagged.push(AutoTagHit {
                category_id,
                name,
                score,
            });
        }

        info!(
            "clotho.auto_tag({memory_id}): {} tag(s) from {} match(es) over threshold {threshold:.2}",
            outcome.tagged.len(),
            matched.len()
        );
        Ok(outcome)
    }

    /// Grow-and-tag pass: match each memory against the LIVE dictionary by
    /// cosine; on a miss, mint a fitting category via the LLM (charter-permitting
    /// — auto-add by default here), add it to the dictionary, and tag — so the
    /// next similar memory reuses it instead of minting again. This is how a
    /// category layer accretes over the flat graph from the corpus itself.
    pub async fn grow_pass(
        &self,
        memories: &[(String, String)],
        threshold: f64,
    ) -> Result<GrowStats, ToolingError> {
        let mut stats = GrowStats::default();
        let cc = self.tooling.config.moira.clotho.clone();

        // Load the live dictionary and embed it once (name: description). This
        // is what lets minted categories be reused: their name+description
        // persist, and the next pass re-embeds them (DB vectors aren't readable).
        let mut dict: Vec<(String, String, Vec<f32>)> = Vec::new();
        for (id, name, desc) in self.tooling.list_categories_full(cc.dict_load_cap).await? {
            let text = if desc.trim().is_empty() {
                name.clone()
            } else {
                format!("{name}: {desc}")
            };
            if let Ok(v) = self.tooling.embed_text(&text).await {
                dict.push((id, name, v));
            }
        }

        for (mem_id, content) in memories {
            stats.scanned += 1;
            if content.trim().is_empty() {
                continue;
            }
            let cv = match self.tooling.embed_text(content).await {
                Ok(v) => v,
                Err(_) => {
                    stats.failed += 1;
                    continue;
                }
            };

            // Dominance gate: score every category, then tag those within
            // DOMINANCE_MARGIN of the best AND over the floor. Multi-tag for
            // genuine multi-domain membership (the overlaps Lachesis routes
            // over), but NOT everything that merely grazes the threshold — that
            // noise-floor smear is what wove the spurious cross-domain bridges.
            let scored: Vec<(&String, f64)> = dict
                .iter()
                .map(|(cid, _, ev)| (cid, cosine(&cv, ev)))
                .collect();
            let best = scored.iter().map(|(_, s)| *s).fold(f64::MIN, f64::max);
            let matches: Vec<(String, i64)> = scored
                .iter()
                .filter(|(_, s)| *s >= threshold && *s >= best - cc.dominance_margin)
                .map(|(cid, s)| ((*cid).clone(), (s.clamp(0.0, 1.0) * 100.0).round() as i64))
                .collect();

            if !matches.is_empty() {
                for (cid, conf) in &matches {
                    let _ = self
                        .tooling
                        .tag_memory(mem_id, cid, *conf, "clotho-embed")
                        .await;
                }
                stats.tagged_by_match += 1;
                continue;
            }

            // Miss → mint a category via the LLM, tag, and add it to the dict.
            match self.mint_category(content).await {
                Ok(Some((cid, name, emb))) => {
                    let _ = self
                        .tooling
                        .tag_memory(mem_id, &cid, cc.mint_confidence, "clotho-llm-mint")
                        .await;
                    if dict.iter().any(|(id, _, _)| id == &cid) {
                        stats.reused_mint += 1;
                    } else {
                        stats.minted += 1;
                        dict.push((cid, name, emb));
                    }
                }
                _ => stats.failed += 1,
            }
        }

        info!(
            "clotho.grow_pass: scanned={} matched={} minted={} reused={} failed={}",
            stats.scanned, stats.tagged_by_match, stats.minted, stats.reused_mint, stats.failed
        );
        Ok(stats)
    }

    /// Ask the LLM for a BROAD, reusable category for `content`, create it
    /// (idempotent) and return `(id, name, embedding)`. The "general/reusable"
    /// instruction is the guard against category explosion — many memories
    /// should share each minted category.
    async fn mint_category(
        &self,
        content: &str,
    ) -> Result<Option<(String, String, Vec<f32>)>, ToolingError> {
        const SYS: &str = "You are Clotho, a librarian maintaining a small controlled vocabulary \
            of BROAD, reusable domain categories for a memory graph. Given a memory, return the \
            single best category that MANY related memories could also share. Strongly prefer \
            general domains (e.g. \"graph databases\", \"software testing\", \"distributed \
            systems\", \"information retrieval\") over narrow, memory-specific labels. Respond \
            with JSON only: {\"category\": \"<short lowercase english noun phrase>\", \
            \"description\": \"<one short sentence>\"}.";

        let (raw, _meta) = self
            .tooling
            .llm_provider
            .generate(SYS, content, Some("json_object"))
            .await
            .map_err(|e| ToolingError::Extraction(e.to_string()))?;
        let v: serde_json::Value = serde_json::from_str(raw.trim())
            .map_err(|e| ToolingError::Extraction(format!("mint parse: {e}; raw={raw}")))?;
        let name = v
            .get("category")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if name.is_empty() {
            return Ok(None);
        }
        let desc = v
            .get("description")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let cid = self
            .tooling
            .ensure_category(&name, "concept", &desc)
            .await?;
        let text = if desc.is_empty() {
            name.clone()
        } else {
            format!("{name}: {desc}")
        };
        let emb = self.tooling.embed_text(&text).await.unwrap_or_default();
        Ok(Some((cid, name, emb)))
    }
}

/// Cosine similarity of two equal-length vectors; `0.0` on length mismatch or a
/// zero-norm vector.
fn cosine(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0f64, 0f64, 0f64);
    for (x, y) in a.iter().zip(b.iter()) {
        let (x, y) = (*x as f64, *y as f64);
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical_is_one() {
        let v = [0.3f32, 0.4, 0.5];
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-9);
    }

    #[test]
    fn cosine_degenerate_is_zero() {
        assert_eq!(cosine(&[1.0, 2.0], &[1.0]), 0.0); // length mismatch
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0); // zero-norm vector
        assert_eq!(cosine(&[], &[]), 0.0); // empty
    }

    #[test]
    fn ancestors_walk_the_seed_hierarchy() {
        assert_eq!(dictionary::ancestors("agriculture"), vec!["raw material"]);
        assert!(
            dictionary::ancestors("raw material").is_empty(),
            "root has no ancestors"
        );
        assert!(dictionary::ancestors("nonexistent-xyz").is_empty());
    }
}
