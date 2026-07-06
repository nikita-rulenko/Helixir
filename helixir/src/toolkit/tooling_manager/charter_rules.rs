//! #34 increment 2b: learned charter rules.
//!
//! Every `resolve_contradiction` verdict is a PRECEDENT — a human-grade
//! judgement the memory would otherwise forget. This module records each
//! one as an episode memory (under `user_id=helixir`, with SUPPORTS edges
//! to both disputed memories), and when enough identical verdicts pile up
//! it PROPOSES a standing rule back to the agent. Adoption is explicit —
//! the agent (or its human) writes the rule memory; nothing self-adopts.
//! The constitution (`memory-charter.md`) is immune to self-learning: rules
//! live beside it in `memory://rules`, never inside it.

use serde::Deserialize;
use tracing::warn;

use super::{ToolingManager, types::ToolingError};

/// Episodes carry this exact `context_tags` value; `searchByContextTag`
/// matches on equality, so the tag IS the counting key.
pub const PRECEDENT_TAG_PREFIX: &str = "charter-precedent:";
/// Adopted rules carry this exact tag; presence of one silences further
/// proposals for the same shape.
pub const RULE_TAG_PREFIX: &str = "charter-rule:";

/// The deterministic shape of a resolved dispute: what kind of new fact met
/// what kind of existing fact, and how the owner settled it. Identical
/// shapes are what "repeated identical answers" means.
#[must_use]
pub fn precedent_shape(new_type: &str, old_type: &str, strategy: &str) -> String {
    fn norm(t: &str) -> String {
        let t = t.trim().to_ascii_lowercase();
        if t.is_empty() {
            "unknown".to_string()
        } else {
            t
        }
    }
    format!("{}-{}-{}", norm(new_type), norm(old_type), norm(strategy))
}

/// The proposal an agent can act on verbatim (pure, unit-tested).
#[must_use]
pub fn suggested_rule_text(shape: &str, count: usize, strategy: &str) -> String {
    let meaning = match strategy {
        "owner_confirmed" => {
            "treat this pair as complementary — keep both records without raising a clarification"
        }
        "owner_retracted" => {
            "let the newer fact supersede the older one automatically (history preserved)"
        }
        _ => "let both records coexist as valid viewpoints without raising a clarification",
    };
    format!(
        "{count} contradiction reviews resolved this shape ({shape}) identically. \
         Proposed standing rule: {meaning}. To adopt it, call \
         add_memory(user_id=\"helixir\", message=\"Charter rule [{shape}]: {meaning}, \
         because {count} reviews resolved this shape the same way.\") — adopted rules \
         appear in the memory://rules resource. The constitution itself never \
         self-learns; only these rules do."
    )
}

/// What `resolve_contradiction` returns to the agent when a rule is ripe.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RuleProposal {
    pub shape: String,
    pub precedents: usize,
    pub proposal: String,
}

#[derive(Deserialize)]
struct MemRow {
    #[serde(default)]
    memory_type: String,
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
struct MemResp {
    memory: Option<MemRow>,
}

#[derive(Deserialize)]
struct TagRows {
    #[serde(default)]
    memories: Vec<serde_json::Value>,
}

impl ToolingManager {
    async fn count_by_exact_tag(&self, tag: &str) -> usize {
        match self
            .db
            .execute_query::<TagRows, _>(
                "searchByContextTag",
                &serde_json::json!({"tag": tag, "limit": 500}),
            )
            .await
        {
            Ok(rows) => rows.memories.len(),
            Err(e) => {
                warn!("charter precedent count failed for tag {tag}: {e}");
                0
            }
        }
    }

    async fn memory_type_and_snippet(&self, memory_id: &str) -> (String, String) {
        match self
            .db
            .execute_query::<MemResp, _>("getMemory", &serde_json::json!({"memory_id": memory_id}))
            .await
        {
            Ok(MemResp { memory: Some(m) }) => {
                let snip: String = m.content.chars().take(80).collect();
                (m.memory_type, snip)
            }
            _ => ("unknown".to_string(), String::new()),
        }
    }

    /// Record one resolved dispute as a precedent episode and, when enough
    /// identical precedents exist with no adopted rule, return a proposal.
    /// Best-effort by design: a failure here must never fail the resolve.
    pub async fn record_charter_precedent(
        &self,
        from_id: &str,
        to_id: &str,
        strategy: &str,
    ) -> Option<RuleProposal> {
        let threshold = self.config.write.rule_propose_after;
        if threshold == 0 {
            return None;
        }

        let (new_type, new_snip) = self.memory_type_and_snippet(from_id).await;
        let (old_type, old_snip) = self.memory_type_and_snippet(to_id).await;
        let shape = precedent_shape(&new_type, &old_type, strategy);
        let episode_tag = format!("{PRECEDENT_TAG_PREFIX}{shape}");
        let rule_tag = format!("{RULE_TAG_PREFIX}{shape}");

        // The episode: a plain recallable account of the judgement.
        let text = format!(
            "Charter precedent ({strategy}): a new {new_type} \"{new_snip}\" was reviewed \
             against existing {old_type} \"{old_snip}\" and the owner resolved it as {strategy}."
        );
        let episode: Result<(), ToolingError> = async {
            let vector = self
                .embedder
                .generate(&text, true)
                .await
                .map_err(|e| ToolingError::Embedding(e.to_string()))?;
            let memory = crate::llm::extractor::ExtractedMemory {
                text: text.clone(),
                memory_type: "fact".to_string(),
                certainty: 95,
                importance: 55,
                entities: vec![],
                context: None,
            };
            let (episode_id, _) = self
                .store_new_memory(&memory, "helixir", &vector, &episode_tag)
                .await?;
            // Provenance: the episode SUPPORTS-links both disputed memories,
            // so a future "why does this rule exist" walks to the evidence.
            for target in [from_id, to_id] {
                let _ = self
                    .reasoning_engine
                    .add_relation(
                        &episode_id,
                        target,
                        crate::toolkit::mind_toolbox::reasoning::ReasoningType::Supports,
                        70,
                        Some("charter-precedent"),
                    )
                    .await;
            }
            Ok(())
        }
        .await;
        if let Err(e) = episode {
            warn!("charter precedent episode store failed (non-fatal): {e}");
            return None;
        }

        let precedents = self.count_by_exact_tag(&episode_tag).await;
        if precedents < threshold {
            return None;
        }
        // A standing rule silences the proposal. Authoritative check is by
        // CONTENT ("Charter rule [shape]"), same as the resource render —
        // the tag is a fast path that dedup can legitimately skip (verified
        // live: an adopted rule NOOP-deduped and carried no tag).
        let shape_marker = format!("[{shape}]");
        let already_adopted = self.count_by_exact_tag(&rule_tag).await > 0
            || self
                .learned_charter_rules()
                .await
                .iter()
                .any(|r| r.contains(&shape_marker));
        if already_adopted {
            return None;
        }
        Some(RuleProposal {
            proposal: suggested_rule_text(&shape, precedents, strategy),
            shape,
            precedents,
        })
    }

    /// Precedent episode counts grouped by shape — the review surface for
    /// `helixir charter` ("N more identical verdicts to a proposal").
    pub async fn charter_precedent_counts(&self) -> Vec<(String, usize)> {
        #[derive(Deserialize)]
        struct Row {
            #[serde(default)]
            content: String,
            #[serde(default)]
            context_tags: String,
        }
        #[derive(Deserialize)]
        struct Rows {
            #[serde(default)]
            memories: Vec<Row>,
        }
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        if let Ok(rows) = self
            .db
            .execute_query::<Rows, _>(
                "searchMemoriesByBm25",
                &serde_json::json!({"text": "Charter precedent", "limit": 500}),
            )
            .await
        {
            for r in rows.memories {
                if r.content.starts_with("Charter precedent")
                    && r.context_tags.starts_with(PRECEDENT_TAG_PREFIX)
                {
                    let shape = r.context_tags[PRECEDENT_TAG_PREFIX.len()..].to_string();
                    *counts.entry(shape).or_insert(0) += 1;
                }
            }
        }
        let mut v: Vec<(String, usize)> = counts.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }

    /// Adopted learned rules, for the `memory://rules` resource — rendered
    /// beside (never inside) the constitution.
    pub async fn learned_charter_rules(&self) -> Vec<String> {
        #[derive(Deserialize)]
        struct Row {
            #[serde(default)]
            content: String,
            #[serde(default)]
            context_tags: String,
        }
        #[derive(Deserialize)]
        struct Rows {
            #[serde(default)]
            memories: Vec<Row>,
        }
        // Adopted rules are written through add_memory with the literal
        // prefix "Charter rule [shape]: ..." (the proposal dictates it), and
        // the write path stamps the exact rule tag when it sees that prefix.
        // BM25 finds them by the prefix tokens; the filter below is the
        // authoritative check.
        match self
            .db
            .execute_query::<Rows, _>(
                "searchMemoriesByBm25",
                &serde_json::json!({"text": "Charter rule", "limit": 50}),
            )
            .await
        {
            Ok(rows) => rows
                .memories
                .into_iter()
                .filter(|r| {
                    r.content.starts_with("Charter rule [")
                        || r.context_tags.starts_with(RULE_TAG_PREFIX)
                })
                .map(|r| r.content)
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_is_normalized_and_deterministic() {
        assert_eq!(
            precedent_shape("Fact", " GOAL ", "owner_confirmed"),
            "fact-goal-owner_confirmed"
        );
        assert_eq!(precedent_shape("", "goal", "x"), "unknown-goal-x");
    }

    #[test]
    fn proposal_text_carries_shape_count_and_adoption_recipe() {
        let p = suggested_rule_text("fact-goal-owner_confirmed", 3, "owner_confirmed");
        assert!(p.contains("3 contradiction reviews"));
        assert!(p.contains("fact-goal-owner_confirmed"));
        assert!(p.contains("add_memory(user_id=\"helixir\""));
        assert!(p.contains("never"));
        assert!(p.contains("self-learns"));
        // Retraction maps to supersede semantics, confirmation to coexistence.
        assert!(
            suggested_rule_text("a-b-owner_retracted", 4, "owner_retracted").contains("supersede")
        );
        assert!(
            suggested_rule_text("a-b-owner_confirmed", 4, "owner_confirmed")
                .contains("complementary")
        );
    }
}
