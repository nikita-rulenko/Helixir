use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{debug, info, warn};

use super::models::{MemoryDecision, MemoryOperation, SimilarMemory};
use super::prompt::{SYSTEM_PROMPT, build_decision_prompt};
use crate::llm::providers::base::LlmProvider;

pub struct LLMDecisionEngine {
    llm: Arc<dyn LlmProvider>,
    similarity_threshold: f64,
    exact_duplicate_score: f64,
    retry_count: AtomicUsize,
    fallback_count: AtomicUsize,
    total_decisions: AtomicUsize,
}

impl LLMDecisionEngine {
    pub fn new(llm: Arc<dyn LlmProvider>) -> Self {
        Self::with_thresholds(llm, 0.70, 0.98)
    }

    pub fn with_thresholds(
        llm: Arc<dyn LlmProvider>,
        similarity_threshold: f64,
        exact_duplicate_score: f64,
    ) -> Self {
        info!(
            "LLMDecisionEngine initialized: provider={}, similarity_threshold={}, exact_duplicate_score={}",
            llm.provider_name(),
            similarity_threshold,
            exact_duplicate_score
        );

        Self {
            llm,
            similarity_threshold,
            exact_duplicate_score,
            retry_count: AtomicUsize::new(0),
            fallback_count: AtomicUsize::new(0),
            total_decisions: AtomicUsize::new(0),
        }
    }

    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.similarity_threshold = threshold;
        self
    }

    pub fn metrics(&self) -> (usize, usize, usize) {
        (
            self.retry_count.load(Ordering::Relaxed),
            self.fallback_count.load(Ordering::Relaxed),
            self.total_decisions.load(Ordering::Relaxed),
        )
    }

    fn validate_decision(
        &self,
        decision: &mut MemoryDecision,
        similar_memories: &[SimilarMemory],
    ) -> bool {
        if decision.confidence > 100 {
            warn!(
                "Confidence {} out of range, clamping to 100",
                decision.confidence
            );
            decision.confidence = 100;
        }

        let needs_target = matches!(
            decision.operation,
            MemoryOperation::Update
                | MemoryOperation::Supersede
                | MemoryOperation::Delete
                | MemoryOperation::Contradict
        );

        if needs_target {
            let highest = similar_memories.iter().max_by(|a, b| {
                a.score
                    .partial_cmp(&b.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            // The wire format keeps operation-specific aliases for model
            // clarity, but execution and charter enforcement must share one
            // canonical target. Prefer the operation-specific field, validate
            // it against the recalled candidates, then mirror it back.
            let requested = match decision.operation {
                MemoryOperation::Supersede => decision
                    .supersedes_memory_id
                    .clone()
                    .or_else(|| decision.target_memory_id.clone()),
                MemoryOperation::Contradict => decision
                    .contradicts_memory_id
                    .clone()
                    .or_else(|| decision.target_memory_id.clone()),
                _ => decision.target_memory_id.clone(),
            };
            let canonical = if requested.is_none() {
                if let Some(best) = highest {
                    warn!(
                        "Operation {:?} missing target, using highest-scoring similar memory: {}",
                        decision.operation, best.id
                    );
                    Some(best.id.clone())
                } else {
                    warn!(
                        "Operation {:?} requires target but no similar memories available, falling back to ADD",
                        decision.operation
                    );
                    return false;
                }
            } else if let Some(ref id) = requested {
                let exists = similar_memories.iter().any(|m| m.id == *id);
                if !exists {
                    warn!("operation target '{}' not found in similar memories", id);
                    if let Some(best) = highest {
                        warn!("Replacing with highest-scoring similar memory: {}", best.id);
                        Some(best.id.clone())
                    } else {
                        return false;
                    }
                } else {
                    requested
                }
            } else {
                None
            };

            decision.target_memory_id = canonical.clone();
            match decision.operation {
                MemoryOperation::Supersede => decision.supersedes_memory_id = canonical,
                MemoryOperation::Contradict => decision.contradicts_memory_id = canonical,
                _ => {}
            }
        }

        true
    }

    /// W2 deterministic gates (#32). Returns `Some(decision)` when no model
    /// of any kind is needed; `None` means the gray zone — consult the LLM
    /// with the returned candidates via `decide`/`decide_batch`.
    pub(crate) fn gate(
        &self,
        new_memory: &str,
        memory_type: &str,
        similar_memories: &[SimilarMemory],
    ) -> Result<MemoryDecision, Vec<SimilarMemory>> {
        if similar_memories.is_empty() {
            debug!("No similar memories, quick ADD");
            return Ok(MemoryDecision::add(
                100,
                "No similar memories found, adding as new.",
            ));
        }

        // Exact-match: byte-identical content (agent retries, double-fires)
        // is a guaranteed-safe NOOP.
        if let Some(same) = similar_memories
            .iter()
            .find(|m| m.content.trim() == new_memory.trim())
        {
            info!(
                "Exact-match gate: content identical to {} — NOOP (no LLM call)",
                same.id
            );
            return Ok(MemoryDecision {
                operation: MemoryOperation::Noop,
                target_memory_id: Some(same.id.clone()),
                confidence: 100,
                reasoning: "exact-match gate: byte-identical content".to_string(),
                ..Default::default()
            });
        }

        let highly_similar: Vec<_> = similar_memories
            .iter()
            .filter(|m| m.score >= self.similarity_threshold)
            .cloned()
            .collect();

        if highly_similar.is_empty() {
            debug!("No memories above threshold {}", self.similarity_threshold);
            return Ok(MemoryDecision::add(
                95,
                format!(
                    "No memories above {} similarity threshold, adding as new.",
                    self.similarity_threshold
                ),
            ));
        }

        // Cosine gate: a near-verbatim duplicate needs no LLM judgement.
        // Everything between similarity_threshold and exact_duplicate_score
        // is the gray zone — numbers and negations barely move embeddings,
        // so it belongs to the LLM.
        //
        // PROTECTED TYPES NEVER COSINE-GATE (charter C3): "prefer dark
        // theme" vs "prefer light theme" embeds at ~0.98+ — a one-word flip
        // the gate would silently NOOP, swallowing a change of mind. Caught
        // live by mcp_write_e2e. Only the byte-exact gate above applies.
        if crate::core::charter::PROTECTED_TYPES.contains(&memory_type) {
            return Err(highly_similar);
        }
        if let Some(top) = highly_similar.iter().max_by(|a, b| {
            a.score
                .partial_cmp(&b.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) && top.score >= self.exact_duplicate_score
        {
            info!(
                "Cosine gate: {:.3} >= {} — NOOP duplicate of {} (no LLM call)",
                top.score, self.exact_duplicate_score, top.id
            );
            return Ok(MemoryDecision {
                operation: MemoryOperation::Noop,
                target_memory_id: Some(top.id.clone()),
                confidence: 98,
                reasoning: format!(
                    "cosine gate: {:.3} >= {} (exact duplicate)",
                    top.score, self.exact_duplicate_score
                ),
                ..Default::default()
            });
        }

        Err(highly_similar)
    }

    pub async fn decide(
        &self,
        new_memory: &str,
        memory_type: &str,
        similar_memories: &[SimilarMemory],
        user_id: &str,
    ) -> MemoryDecision {
        self.total_decisions.fetch_add(1, Ordering::Relaxed);

        debug!(
            "Making decision: new_memory='{}...', similar_count={}",
            crate::safe_truncate(new_memory, 50),
            similar_memories.len()
        );

        let highly_similar = match self.gate(new_memory, memory_type, similar_memories) {
            Ok(decision) => return decision,
            Err(gray) => gray,
        };

        let prompt = build_decision_prompt(new_memory, &highly_similar, user_id);

        debug!(
            "Calling LLM for decision with {} candidates",
            highly_similar.len()
        );

        match self
            .llm
            .generate(SYSTEM_PROMPT, &prompt, Some("json_object"))
            .await
        {
            Ok((response, _metadata)) => match serde_json::from_str::<MemoryDecision>(&response) {
                Ok(mut decision) => {
                    if !self.validate_decision(&mut decision, &highly_similar) {
                        self.fallback_count.fetch_add(1, Ordering::Relaxed);
                        return MemoryDecision::add(50, "Validation failed, defaulting to ADD.");
                    }
                    info!(
                        "Decision made: operation={:?}, confidence={}, target={:?}",
                        decision.operation, decision.confidence, decision.target_memory_id
                    );
                    decision
                }
                Err(e) => {
                    warn!("Failed to parse LLM response as JSON: {}", e);
                    warn!("Response was: {}", crate::safe_truncate(&response, 200));
                    self.retry_count.fetch_add(1, Ordering::Relaxed);

                    let retry_prompt = format!(
                        "{}\n\nIMPORTANT: Your previous response was not valid JSON. Error: {}. Output ONLY valid JSON with no markdown fences, no explanation.",
                        prompt, e
                    );

                    match self
                        .llm
                        .generate(SYSTEM_PROMPT, &retry_prompt, Some("json_object"))
                        .await
                    {
                        Ok((retry_response, _)) => {
                            match serde_json::from_str::<MemoryDecision>(&retry_response) {
                                Ok(mut decision) => {
                                    info!("Retry succeeded for JSON parse");
                                    if !self.validate_decision(&mut decision, &highly_similar) {
                                        self.fallback_count.fetch_add(1, Ordering::Relaxed);
                                        return MemoryDecision::add(
                                            50,
                                            "Validation failed after retry, defaulting to ADD.",
                                        );
                                    }
                                    info!(
                                        "Decision made (after retry): operation={:?}, confidence={}, target={:?}",
                                        decision.operation,
                                        decision.confidence,
                                        decision.target_memory_id
                                    );
                                    decision
                                }
                                Err(e2) => {
                                    warn!("Retry also failed to parse JSON: {}", e2);
                                    self.fallback_count.fetch_add(1, Ordering::Relaxed);
                                    MemoryDecision::add(
                                        50,
                                        format!(
                                            "JSON parse failed after retry ({}), defaulting to ADD.",
                                            e2
                                        ),
                                    )
                                }
                            }
                        }
                        Err(e2) => {
                            warn!("Retry LLM call failed: {}", e2);
                            self.fallback_count.fetch_add(1, Ordering::Relaxed);
                            MemoryDecision::add(
                                50,
                                format!("Retry LLM call failed ({}), defaulting to ADD.", e2),
                            )
                        }
                    }
                }
            },
            Err(e) => {
                warn!("LLM call failed: {}", e);
                self.fallback_count.fetch_add(1, Ordering::Relaxed);
                MemoryDecision::add(50, format!("LLM call failed ({}), defaulting to ADD.", e))
            }
        }
    }

    /// W1 (#32): one LLM call decides every gray-zone item of a batch.
    /// Gated items (exact/cosine/threshold) never reach the model. On batch
    /// parse failure the gray items fall back to per-item `decide`.
    pub async fn decide_batch(
        &self,
        items: &[(String, String, Vec<SimilarMemory>)],
        user_id: &str,
    ) -> Vec<MemoryDecision> {
        let mut decisions: Vec<Option<MemoryDecision>> = vec![None; items.len()];
        let mut gray: Vec<(usize, &str, Vec<SimilarMemory>)> = Vec::new();

        for (i, (new_memory, memory_type, similar)) in items.iter().enumerate() {
            self.total_decisions.fetch_add(1, Ordering::Relaxed);
            match self.gate(new_memory, memory_type, similar) {
                Ok(decision) => decisions[i] = Some(decision),
                Err(highly_similar) => gray.push((i, new_memory.as_str(), highly_similar)),
            }
        }

        if !gray.is_empty() {
            info!(
                "Batch decision: {} gray-zone item(s) in ONE LLM call ({} gated)",
                gray.len(),
                items.len() - gray.len()
            );
            // #96 Lever 1.5: the prompt shows DENSE LOCAL indices 0..n-1.
            // Gating makes the original indices sparse, and models renumber
            // sparse lists — every mismatched index used to dump an item into
            // a per-item call (N extra calls; measured costlier than the
            // whole infer phase). Local index -> original slot via `gray`.
            let prompt_items: Vec<(usize, &str, &[SimilarMemory])> = gray
                .iter()
                .enumerate()
                .map(|(local, (_, text, cands))| (local, *text, cands.as_slice()))
                .collect();
            let base_prompt = super::prompt::build_batch_decision_prompt(&prompt_items, user_id);

            #[derive(serde::Deserialize)]
            struct BatchItem {
                i: usize,
                #[serde(flatten)]
                decision: MemoryDecision,
            }
            #[derive(serde::Deserialize)]
            struct BatchResponse {
                decisions: Vec<BatchItem>,
            }

            // Two batched attempts (initial + ONE repair naming the missing
            // items) before any per-item fallback: a repair retry costs one
            // call, N per-item fallbacks cost N.
            let mut prompt = base_prompt.clone();
            for attempt in 0..2u8 {
                let parsed: Option<BatchResponse> = match self
                    .llm
                    .generate(SYSTEM_PROMPT, &prompt, Some("json_object"))
                    .await
                {
                    Ok((response, _)) => match serde_json::from_str(&response) {
                        Ok(batch) => Some(batch),
                        Err(e) => {
                            // The raw body is the diagnosis — a blind
                            // fallback here costs more than the whole
                            // infer phase (#96).
                            warn!(
                                "Batch decision parse failed (attempt {attempt}): {e}; response: {}",
                                crate::safe_truncate(&response, 600)
                            );
                            None
                        }
                    },
                    Err(e) => {
                        warn!("Batch decision LLM call failed (attempt {attempt}): {e}");
                        None
                    }
                };

                if let Some(batch) = parsed {
                    for item in batch.decisions {
                        let Some((orig, _, highly_similar)) = gray.get(item.i) else {
                            warn!(
                                "Batch decision: response index {} out of range (0..{})",
                                item.i,
                                gray.len()
                            );
                            continue;
                        };
                        let mut decision = item.decision;
                        if self.validate_decision(&mut decision, highly_similar)
                            && decisions.get(*orig).is_some_and(Option::is_none)
                        {
                            decisions[*orig] = Some(decision);
                        }
                    }
                }

                let missing: Vec<usize> = gray
                    .iter()
                    .enumerate()
                    .filter(|(_, (orig, _, _))| decisions[*orig].is_none())
                    .map(|(local, _)| local)
                    .collect();
                if missing.is_empty() {
                    break;
                }
                if attempt == 0 {
                    self.retry_count.fetch_add(1, Ordering::Relaxed);
                    info!(
                        "Batch decision: {} item(s) unresolved after attempt 0, one batched repair",
                        missing.len()
                    );
                    prompt = format!(
                        "{base_prompt}\n\nIMPORTANT: your previous response was invalid or \
                         incomplete for item number(s) {missing:?}. Respond again with the \
                         COMPLETE decisions array — every item number from 0 to {} exactly \
                         once, valid JSON only, no markdown fences.",
                        gray.len() - 1
                    );
                }
            }
        }

        // Anything unresolved (batch failure, missing index, validation
        // reject) falls back to the per-item path — correctness over savings.
        let mut result = Vec::with_capacity(items.len());
        for (i, slot) in decisions.into_iter().enumerate() {
            match slot {
                Some(d) => result.push(d),
                None => {
                    warn!("Batch decision: item {i} unresolved, per-item fallback");
                    let (new_memory, memory_type, similar) = &items[i];
                    result.push(self.decide(new_memory, memory_type, similar, user_id).await);
                }
            }
        }
        result
    }

    pub fn is_likely_duplicate(&self, similar_memories: &[SimilarMemory]) -> bool {
        similar_memories
            .iter()
            .any(|m| m.score >= self.exact_duplicate_score)
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
