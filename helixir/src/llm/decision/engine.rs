

use std::sync::Arc;
use tracing::{debug, info, warn};

use super::models::{MemoryDecision, MemoryOperation, SimilarMemory};
use super::prompt::{build_decision_prompt, SYSTEM_PROMPT};
use crate::llm::providers::base::LlmProvider;


pub struct LLMDecisionEngine {
    
    llm: Arc<dyn LlmProvider>,
    
    similarity_threshold: f64,
}

impl LLMDecisionEngine {
    
    pub fn new(llm: Arc<dyn LlmProvider>) -> Self {
        info!(
            "LLMDecisionEngine initialized: provider={}",
            llm.provider_name()
        );

        Self {
            llm,
            similarity_threshold: 0.92,
        }
    }

    
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.similarity_threshold = threshold;
        self
    }

    
    pub async fn decide(
        &self,
        new_memory: &str,
        similar_memories: &[SimilarMemory],
        user_id: &str,
    ) -> MemoryDecision {
        debug!(
            "Making decision: new_memory='{}...', similar_count={}",
            crate::safe_truncate(&new_memory, 50),
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
                    Ok(decision) => {
                        info!(
                            "Decision made: operation={:?}, confidence={}, target={:?}",
                            decision.operation, decision.confidence, decision.target_memory_id
                        );
                        decision
                    }
                    Err(e) => {
                        warn!("Failed to parse LLM response as JSON: {}", e);
                        warn!("Response was: {}", crate::safe_truncate(&response, 200));
                        MemoryDecision::add(50, format!("JSON parse failed ({}), defaulting to ADD.", e))
                    }
                }
            }
            Err(e) => {
                warn!("LLM call failed: {}", e);
                MemoryDecision::add(50, format!("LLM call failed ({}), defaulting to ADD.", e))
            }
        }
    }

    
    pub fn is_likely_duplicate(&self, similar_memories: &[SimilarMemory]) -> bool {
        similar_memories
            .iter()
            .any(|m| m.score >= 0.98) 
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
}