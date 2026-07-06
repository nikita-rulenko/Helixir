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

            if decision.target_memory_id.is_none() {
                if let Some(best) = highest {
                    warn!(
                        "Operation {:?} missing target_memory_id, using highest-scoring similar memory: {}",
                        decision.operation, best.id
                    );
                    decision.target_memory_id = Some(best.id.clone());
                } else {
                    warn!(
                        "Operation {:?} requires target but no similar memories available, falling back to ADD",
                        decision.operation
                    );
                    return false;
                }
            } else if let Some(ref id) = decision.target_memory_id {
                let exists = similar_memories.iter().any(|m| m.id == *id);
                if !exists {
                    warn!("target_memory_id '{}' not found in similar memories", id);
                    if let Some(best) = highest {
                        warn!("Replacing with highest-scoring similar memory: {}", best.id);
                        decision.target_memory_id = Some(best.id.clone());
                    }
                }
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
        }) {
            if top.score >= self.exact_duplicate_score {
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
mod tests {
    use super::*;

    /// Proves a code path makes no LLM call: panics if consulted.
    struct PanicProvider;

    #[async_trait::async_trait]
    impl crate::llm::providers::base::LlmProvider for PanicProvider {
        async fn generate(
            &self,
            _system_prompt: &str,
            _user_prompt: &str,
            _response_format: Option<&str>,
        ) -> Result<
            (String, crate::llm::providers::base::LlmMetadata),
            crate::llm::providers::base::LlmProviderError,
        > {
            panic!("LLM must not be consulted on a gated decision");
        }

        fn provider_name(&self) -> &str {
            "panic"
        }

        fn model_name(&self) -> &str {
            "panic"
        }
    }

    fn gated_engine() -> LLMDecisionEngine {
        LLMDecisionEngine::with_thresholds(
            std::sync::Arc::new(PanicProvider)
                as std::sync::Arc<dyn crate::llm::providers::base::LlmProvider>,
            0.70,
            0.98,
        )
    }

    fn similar(id: &str, score: f64) -> SimilarMemory {
        SimilarMemory {
            id: id.to_string(),
            content: "same fact".to_string(),
            score,
            memory_type: None,
            created_at: None,
            user_id: None,
            is_cross_user: false,
        }
    }

    #[tokio::test]
    async fn cosine_gates_skip_llm() {
        let engine = gated_engine();

        // Upper gate: near-verbatim duplicate -> NOOP, no LLM.
        let d = engine
            .decide("the same fact", "fact", &[similar("mem_dup", 0.99)], "u")
            .await;
        assert_eq!(d.operation, MemoryOperation::Noop);
        assert_eq!(d.target_memory_id.as_deref(), Some("mem_dup"));

        // Lower gate: nothing above the similarity threshold -> ADD, no LLM.
        let d = engine
            .decide("a novel fact", "fact", &[similar("mem_far", 0.42)], "u")
            .await;
        assert_eq!(d.operation, MemoryOperation::Add);

        // No candidates at all -> ADD, no LLM.
        let d = engine.decide("first fact ever", "fact", &[], "u").await;
        assert_eq!(d.operation, MemoryOperation::Add);

        // Exact string match -> NOOP regardless of the (blended) score.
        let d = engine
            .decide("same fact", "fact", &[similar("mem_same", 0.80)], "u")
            .await;
        assert_eq!(d.operation, MemoryOperation::Noop);
        assert_eq!(d.target_memory_id.as_deref(), Some("mem_same"));

        // Protected types never cosine-gate: a 0.99 "duplicate" preference
        // may be a one-word reversal — must reach the LLM (gray zone).
        let gray = engine.gate(
            "the user prefers light theme",
            "preference",
            &[similar("mem_pref", 0.99)],
        );
        assert!(gray.is_err(), "protected type must not be cosine-gated");
    }

    #[test]
    fn test_memory_decision_builders() {
        let add = MemoryDecision::add(100, "test reason");
        assert_eq!(add.operation, MemoryOperation::Add);
        assert_eq!(add.confidence, 100);

        let noop = MemoryDecision::noop(90, "duplicate");
        assert_eq!(noop.operation, MemoryOperation::Noop);

        let update = MemoryDecision::update("mem_123", "merged", 85, "merging");
        assert_eq!(update.operation, MemoryOperation::Update);
        assert_eq!(update.target_memory_id, Some("mem_123".to_string()));
        assert_eq!(update.merged_content, Some("merged".to_string()));

        let supersede = MemoryDecision::supersede("mem_old", 80, "evolved");
        assert_eq!(supersede.operation, MemoryOperation::Supersede);
        assert_eq!(supersede.supersedes_memory_id, Some("mem_old".to_string()));
    }

    #[test]
    fn test_link_existing_builder() {
        let link = MemoryDecision::link_existing("mem_shared", 90, "same fact from different user");
        assert_eq!(link.operation, MemoryOperation::LinkExisting);
        assert_eq!(link.link_to_memory_id, Some("mem_shared".to_string()));
        assert_eq!(link.confidence, 90);
        assert!(link.target_memory_id.is_none());
        assert!(link.conflict_type.is_none());
    }

    #[test]
    fn test_cross_contradict_builder() {
        let cc =
            MemoryDecision::cross_contradict("mem_other", "preference", 85, "opposing preferences");
        assert_eq!(cc.operation, MemoryOperation::CrossContradict);
        assert_eq!(cc.contradicts_memory_id, Some("mem_other".to_string()));
        assert_eq!(cc.conflict_type, Some("preference".to_string()));
        assert_eq!(cc.confidence, 85);
        assert!(cc.link_to_memory_id.is_none());
    }

    #[test]
    fn test_similar_memory_cross_user_fields() {
        let personal = SimilarMemory {
            id: "mem_1".to_string(),
            content: "test".to_string(),
            score: 0.9,
            memory_type: None,
            created_at: None,
            user_id: None,
            is_cross_user: false,
        };
        assert!(!personal.is_cross_user);

        let cross = SimilarMemory {
            id: "mem_2".to_string(),
            content: "test".to_string(),
            score: 0.85,
            memory_type: None,
            created_at: None,
            user_id: Some("other_user".to_string()),
            is_cross_user: true,
        };
        assert!(cross.is_cross_user);
        assert_eq!(cross.user_id, Some("other_user".to_string()));
    }

    #[test]
    fn test_prompt_includes_cross_user_section() {
        use super::super::prompt::build_decision_prompt;

        let cross_memories = vec![SimilarMemory {
            id: "mem_other".to_string(),
            content: "I prefer dark mode".to_string(),
            score: 0.88,
            memory_type: None,
            created_at: Some("2025-01-01T00:00:00Z".to_string()),
            user_id: Some("user_b".to_string()),
            is_cross_user: true,
        }];

        let prompt = build_decision_prompt("I prefer light mode", &cross_memories, "user_a");

        assert!(prompt.contains("LINK_EXISTING"));
        assert!(prompt.contains("CROSS_CONTRADICT"));
        assert!(prompt.contains("DIFFERENT USER"));
        assert!(prompt.contains("link_to_memory_id"));
    }

    #[test]
    fn test_prompt_no_cross_user_section_for_personal() {
        use super::super::prompt::build_decision_prompt;

        let personal_memories = vec![SimilarMemory {
            id: "mem_mine".to_string(),
            content: "Rust is my favorite language".to_string(),
            score: 0.9,
            memory_type: None,
            created_at: None,
            user_id: None,
            is_cross_user: false,
        }];

        let prompt = build_decision_prompt("Rust is great", &personal_memories, "user_a");

        assert!(!prompt.contains("LINK_EXISTING"));
        assert!(!prompt.contains("CROSS_CONTRADICT"));
        assert!(!prompt.contains("DIFFERENT USER"));
    }

    /// #96 Lever 1.5: a scripted provider returning queued responses, with a
    /// call counter — proves batch reliability without a live LLM.
    struct ScriptedProvider {
        responses: std::sync::Mutex<Vec<String>>,
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl crate::llm::providers::base::LlmProvider for ScriptedProvider {
        async fn generate(
            &self,
            _system_prompt: &str,
            _user_prompt: &str,
            _response_format: Option<&str>,
        ) -> Result<
            (String, crate::llm::providers::base::LlmMetadata),
            crate::llm::providers::base::LlmProviderError,
        > {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let mut q = self.responses.lock().unwrap();
            let body = if q.is_empty() {
                panic!("ScriptedProvider exhausted — more LLM calls than the contract allows")
            } else {
                q.remove(0)
            };
            Ok((body, crate::llm::providers::base::LlmMetadata::default()))
        }

        fn provider_name(&self) -> &str {
            "scripted"
        }

        fn model_name(&self) -> &str {
            "scripted"
        }
    }

    fn scripted_engine(responses: Vec<&str>) -> (LLMDecisionEngine, Arc<ScriptedProvider>) {
        let provider = Arc::new(ScriptedProvider {
            responses: std::sync::Mutex::new(responses.into_iter().map(String::from).collect()),
            calls: AtomicUsize::new(0),
        });
        (
            LLMDecisionEngine::with_thresholds(
                Arc::clone(&provider) as Arc<dyn crate::llm::providers::base::LlmProvider>,
                0.70,
                0.98,
            ),
            provider,
        )
    }

    fn batch_items(specs: &[(&str, f64)]) -> Vec<(String, String, Vec<SimilarMemory>)> {
        specs
            .iter()
            .enumerate()
            .map(|(n, (text, score))| {
                (
                    (*text).to_string(),
                    "fact".to_string(),
                    vec![similar(&format!("mem_c{n}"), *score)],
                )
            })
            .collect()
    }

    /// Gating makes the original indices sparse (item 0 gated here), and the
    /// model answers with DENSE indices 0..n-1 — exactly what used to dump
    /// every item into a per-item call. One call must now resolve all three.
    #[tokio::test]
    async fn sparse_gray_indices_resolve_in_one_call() {
        let (engine, provider) = scripted_engine(vec![
            r#"{"decisions":[
                {"i":0,"operation":"ADD","confidence":80,"reasoning":"a"},
                {"i":1,"operation":"ADD","confidence":80,"reasoning":"b"},
                {"i":2,"operation":"UPDATE","target_memory_id":"mem_c3","confidence":85,"reasoning":"c","merged_content":"m"}
            ]}"#,
        ]);
        // Item 0 gated (exact dup), items 1..3 gray -> local indices 0..2.
        let items = batch_items(&[
            ("dup", 0.99),
            ("gray one", 0.85),
            ("gray two", 0.85),
            ("gray three", 0.85),
        ]);
        let decisions = engine.decide_batch(&items, "u").await;

        assert_eq!(
            provider.calls.load(Ordering::Relaxed),
            1,
            "one batched call"
        );
        assert_eq!(decisions[0].operation, MemoryOperation::Noop); // gated
        assert_eq!(decisions[1].operation, MemoryOperation::Add);
        assert_eq!(decisions[2].operation, MemoryOperation::Add);
        assert_eq!(decisions[3].operation, MemoryOperation::Update);
        assert_eq!(decisions[3].target_memory_id.as_deref(), Some("mem_c3"));
    }

    /// An incomplete first response is repaired by ONE batched retry — never
    /// by N per-item calls (a per-item call would exhaust the script and
    /// panic, and the call counter pins the total at 2).
    #[tokio::test]
    async fn incomplete_batch_repairs_in_one_batched_retry() {
        let (engine, provider) = scripted_engine(vec![
            r#"{"decisions":[{"i":0,"operation":"ADD","confidence":80,"reasoning":"only one"}]}"#,
            r#"{"decisions":[
                {"i":0,"operation":"ADD","confidence":80,"reasoning":"a"},
                {"i":1,"operation":"ADD","confidence":80,"reasoning":"b"},
                {"i":2,"operation":"ADD","confidence":80,"reasoning":"c"}
            ]}"#,
        ]);
        let items = batch_items(&[("g1", 0.85), ("g2", 0.85), ("g3", 0.85)]);
        let decisions = engine.decide_batch(&items, "u").await;

        assert_eq!(
            provider.calls.load(Ordering::Relaxed),
            2,
            "initial + one batched repair, zero per-item calls"
        );
        assert!(
            decisions
                .iter()
                .all(|d| d.operation == MemoryOperation::Add)
        );
    }
}
