//! #96 Lever 2: route SUPPORTS/CONTRADICTS off the LLM.
//!
//! Relation inference (Phase D) pays an LLM call to type edges — but the
//! two most common inferred types are exactly what a local NLI
//! cross-encoder decides deterministically for pennies of CPU: entailment
//! (the new fact SUPPORTS the existing one) and contradiction. Route those
//! pairs through the already-shipped deberta judge; only the residual
//! pairs — the ones that might hide IMPLICIT causality (BECAUSE/IMPLIES) —
//! still go to the model. On many writes that residual is empty and the
//! infer call disappears entirely.
//!
//! Self-gating: lean release artifacts build without the `nli` feature and
//! a source build may lack the downloaded model — in both cases the router
//! is a no-op and every pair flows to the LLM as before.

use crate::toolkit::mind_toolbox::reasoning::ReasoningRelation;
#[cfg(feature = "nli")]
use crate::toolkit::mind_toolbox::reasoning::ReasoningType;

use super::super::ToolingManager;

/// (new_id, new_content, candidates[(id, content)]) — the Phase D job shape.
pub(super) type InferJob = (String, String, Vec<(String, String)>);

#[cfg(feature = "nli")]
mod judge {
    use std::sync::{Mutex, OnceLock};

    use tracing::{info, warn};

    /// Process-wide lazy judge: the load (ONNX session) is the expensive
    /// part; classify is per-pair CPU. `None` = model missing/unloadable —
    /// decided once, logged once.
    static NLI_JUDGE: OnceLock<Option<Mutex<crate::llm::nli::NliJudge>>> = OnceLock::new();

    pub(super) fn get() -> Option<&'static Mutex<crate::llm::nli::NliJudge>> {
        NLI_JUDGE
            .get_or_init(|| {
                let dir = crate::llm::nli::NliJudge::default_dir();
                match crate::llm::nli::NliJudge::load(&dir) {
                    Ok(j) => {
                        info!("NLI edge router loaded ({})", dir.display());
                        Some(Mutex::new(j))
                    }
                    Err(e) => {
                        warn!(
                            "NLI edge router unavailable ({e}) — all relation \
                             inference stays on the LLM"
                        );
                        None
                    }
                }
            })
            .as_ref()
    }
}

impl ToolingManager {
    /// Split the Phase D jobs: pairs the NLI judge decides confidently come
    /// back as ready relations; everything else returns as residual jobs
    /// for the LLM. Synchronous CPU — call inside `block_in_place`.
    pub(super) fn route_relations_nli(
        &self,
        jobs: Vec<InferJob>,
    ) -> (Vec<ReasoningRelation>, Vec<InferJob>) {
        if !self.config.write.nli_route {
            return (Vec::new(), jobs);
        }
        #[cfg(not(feature = "nli"))]
        {
            return (Vec::new(), jobs);
        }
        #[cfg(feature = "nli")]
        {
            let Some(judge) = judge::get() else {
                return (Vec::new(), jobs);
            };
            let min_prob = self.config.write.nli_route_min_prob;
            let mut routed = Vec::new();
            let mut residual = Vec::new();

            for (new_id, new_content, candidates) in jobs {
                let mut rest = Vec::new();
                for (cand_id, cand_content) in candidates {
                    // BOTH directions, like `is_same_fact`: NLI entailment is
                    // specific⊨general, while a SUPPORTS edge in our graph is
                    // evidence in either orientation ("retries up to five
                    // times" supports "retries automatically" and vice versa
                    // — caught live: one-directional missed the general→
                    // specific pair). Contradiction is symmetric anyway.
                    // Neutral both ways — or unconfident — stays with the
                    // LLM, which may still see implicit BECAUSE/IMPLIES.
                    let verdict = {
                        let mut j = judge.lock().expect("nli judge lock");
                        let fwd = j.classify(&new_content, &cand_content);
                        match fwd {
                            Ok((crate::llm::nli::NliLabel::Neutral, _)) => {
                                j.classify(&cand_content, &new_content)
                            }
                            other => other,
                        }
                    };
                    let (rel_type, prob) = match verdict {
                        Ok((crate::llm::nli::NliLabel::Entailment, p)) if p[1] >= min_prob => {
                            (ReasoningType::Supports, p[1])
                        }
                        // NLI is subject-blind: "the export job never retries"
                        // vs "the daemon retries" scored contradiction 0.99
                        // live — different subjects, no real conflict. Same
                        // cure as #93: a CONTRADICTS route additionally needs
                        // a shared subject; otherwise the pair stays with the
                        // LLM, which does see subjects.
                        Ok((crate::llm::nli::NliLabel::Contradiction, p))
                            if p[0] >= min_prob
                                && crate::core::charter::shares_subject(
                                    &new_content,
                                    &cand_content,
                                ) =>
                        {
                            (ReasoningType::Contradicts, p[0])
                        }
                        _ => {
                            rest.push((cand_id, cand_content));
                            continue;
                        }
                    };
                    routed.push(ReasoningRelation {
                        peer_memory_id: String::new(),
                        peer_memory_content: String::new(),
                        relation_id: format!(
                            "nli_{}_{}",
                            crate::safe_truncate(&new_id, 8),
                            crate::safe_truncate(&cand_id, 8)
                        ),
                        from_memory_id: new_id.clone(),
                        to_memory_id: cand_id,
                        to_memory_content: cand_content,
                        from_memory_content: new_content.clone(),
                        relation_type: rel_type,
                        strength: (prob * 100.0).round().clamp(0.0, 100.0) as i32,
                        reasoning_id: Some("nli_routed".to_string()),
                    });
                }
                if !rest.is_empty() {
                    residual.push((new_id, new_content, rest));
                }
            }
            (routed, residual)
        }
    }
}
