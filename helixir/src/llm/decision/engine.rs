

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{debug, info, warn};

use super::models::{MemoryDecision, MemoryOperation, SimilarMemory};
use super::prompt::{build_decision_prompt, SYSTEM_PROMPT};
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

    pub fn with_thresholds(llm: Arc<dyn LlmProvider>, similarity_threshold: f64, exact_duplicate_score: f64) -> Self {
        info!(
            "LLMDecisionEngine initialized: provider={}, similarity_threshold={}, exact_duplicate_score={}",
            llm.provider_name(), similarity_threshold, exact_duplicate_score
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

    fn validate_decision(&self, decision: &mut MemoryDecision, similar_memories: &[SimilarMemory]) -> bool {
        if decision.confidence > 100 {
            warn!("Confidence {} out of range, clamping to 100", decision.confidence);
            decision.confidence = 100;
        }

        let needs_target = matches!(
            decision.operation,
            MemoryOperation::Update | MemoryOperation::Supersede | MemoryOperation::Delete | MemoryOperation::Contradict
        );

        if needs_target {
            let highest = similar_memories
                .iter()
                .max_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal));

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

    pub async fn decide(
        &self,
        new_memory: &str,
        similar_memories: &[SimilarMemory],
        user_id: &str,
    ) -> MemoryDecision {
        self.total_decisions.fetch_add(1, Ordering::Relaxed);

        debug!(
            "Making decision: new_memory='{}...', similar_count={}",
            crate::safe_truncate(new_memory, 50),
            similar_memories.len()
        );

        if similar_memories.is_empty() {
            debug!("No similar memories, quick ADD");
            return MemoryDecision::add(100, "No similar memories found, adding as new.");
        }

        let highly_similar: Vec<_> = similar_memories
            .iter()
            .filter(|m| m.score >= self.similarity_threshold)
            .cloned()
            .collect();

        if highly_similar.is_empty() {
            debug!("No memories above threshold {}", self.similarity_threshold);
            return MemoryDecision::add(
                95,
                format!(
                    "No memories above {} similarity threshold, adding as new.",
                    self.similarity_threshold
                ),
            );
        }

        let prompt = build_decision_prompt(new_memory, &highly_similar, user_id);

        debug!("Calling LLM for decision with {} candidates", highly_similar.len());

        match self.llm.generate(SYSTEM_PROMPT, &prompt, Some("json_object")).await {
            Ok((response, _metadata)) => {
                match serde_json::from_str::<MemoryDecision>(&response) {
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

                        match self.llm.generate(SYSTEM_PROMPT, &retry_prompt, Some("json_object")).await {
                            Ok((retry_response, _)) => {
                                match serde_json::from_str::<MemoryDecision>(&retry_response) {
                                    Ok(mut decision) => {
                                        info!("Retry succeeded for JSON parse");
                                        if !self.validate_decision(&mut decision, &highly_similar) {
                                            self.fallback_count.fetch_add(1, Ordering::Relaxed);
                                            return MemoryDecision::add(50, "Validation failed after retry, defaulting to ADD.");
                                        }
                                        info!(
                                            "Decision made (after retry): operation={:?}, confidence={}, target={:?}",
                                            decision.operation, decision.confidence, decision.target_memory_id
                                        );
                                        decision
                                    }
                                    Err(e2) => {
                                        warn!("Retry also failed to parse JSON: {}", e2);
                                        self.fallback_count.fetch_add(1, Ordering::Relaxed);
                                        MemoryDecision::add(50, format!("JSON parse failed after retry ({}), defaulting to ADD.", e2))
                                    }
                                }
                            }
                            Err(e2) => {
                                warn!("Retry LLM call failed: {}", e2);
                                self.fallback_count.fetch_add(1, Ordering::Relaxed);
                                MemoryDecision::add(50, format!("Retry LLM call failed ({}), defaulting to ADD.", e2))
                            }
                        }
                    }
                }
            }
            Err(e) => {
                warn!("LLM call failed: {}", e);
                self.fallback_count.fetch_add(1, Ordering::Relaxed);
                MemoryDecision::add(50, format!("LLM call failed ({}), defaulting to ADD.", e))
            }
        }
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
        let cc = MemoryDecision::cross_contradict(
            "mem_other", "preference", 85, "opposing preferences"
        );
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
            created_at: None,
            user_id: None,
            is_cross_user: false,
        };
        assert!(!personal.is_cross_user);

        let cross = SimilarMemory {
            id: "mem_2".to_string(),
            content: "test".to_string(),
            score: 0.85,
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
            created_at: Some("2025-01-01T00:00:00Z".to_string()),
            user_id: Some("user_b".to_string()),
            is_cross_user: true,
        }];

        let prompt = build_decision_prompt(
            "I prefer light mode",
            &cross_memories,
            "user_a",
        );

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
            created_at: None,
            user_id: None,
            is_cross_user: false,
        }];

        let prompt = build_decision_prompt(
            "Rust is great",
            &personal_memories,
            "user_a",
        );

        assert!(!prompt.contains("LINK_EXISTING"));
        assert!(!prompt.contains("CROSS_CONTRADICT"));
        assert!(!prompt.contains("DIFFERENT USER"));
    }
}