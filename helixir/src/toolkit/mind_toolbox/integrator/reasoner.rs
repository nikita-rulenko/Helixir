use std::sync::Arc;
use async_trait::async_trait;
use thiserror::Error;
use tracing::{debug, warn};

use super::models::{SimilarMemory, MemoryRelation, RelationType};

#[derive(Error, Debug)]
pub enum ReasoningError {
    #[error("Reasoning engine failed: {0}")]
    EngineFailed(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

#[derive(Debug, Clone)]
pub struct InferredRelation {
    pub relation_type: RelationType,
    pub confidence: f64,
    pub reasoning: String,
}

#[async_trait]
pub trait ReasoningEngine: Send + Sync {
    async fn infer_relation(
        &self,
        source: &str,
        target: &str,
        similarity: f64,
    ) -> Result<InferredRelation, ReasoningError>;
}

pub struct RelationInferrer {
    reasoning_engine: Option<Arc<dyn ReasoningEngine>>,
    enable_reasoning: bool,
}

impl RelationInferrer {
    pub fn new(engine: Option<Arc<dyn ReasoningEngine>>, enable: bool) -> Self {
        Self {
            reasoning_engine: engine,
            enable_reasoning: enable,
        }
    }

    pub async fn infer_relations(
        &self,
        new_content: &str,
        similar: &[SimilarMemory],
    ) -> Vec<MemoryRelation> {
        if !self.enable_reasoning || self.reasoning_engine.is_none() {
            return self.heuristic_relations(similar);
        }

        let engine = self.reasoning_engine.as_ref().unwrap();
        let mut relations = Vec::new();

        for sim in similar {
            match engine.infer_relation(new_content, &sim.content, sim.similarity_score).await {
                Ok(inferred) => {
                    relations.push(MemoryRelation {
                        target_id: sim.memory_id.clone(),
                        relation_type: inferred.relation_type,
                        confidence: inferred.confidence,
                        reasoning: inferred.reasoning,
                    });
                }
                Err(e) => {
                    warn!("Reasoning failed for similarity: {}", e);
                    relations.push(self.fallback_relation(sim));
                }
            }
        }

        relations
    }

    fn heuristic_relations(&self, similar: &[SimilarMemory]) -> Vec<MemoryRelation> {
        similar
            .iter()
            .filter(|sim| sim.similarity_score >= 0.75)
            .map(|sim| MemoryRelation {
                target_id: sim.memory_id.clone(),
                relation_type: RelationType::RelatesTo,
                confidence: sim.similarity_score,
                reasoning: format!("Semantic similarity: {:.2}", sim.similarity_score),
            })
            .collect()
    }

    fn fallback_relation(&self, sim: &SimilarMemory) -> MemoryRelation {
        MemoryRelation {
            target_id: sim.memory_id.clone(),
            relation_type: RelationType::RelatesTo,
            confidence: sim.similarity_score,
            reasoning: format!("Fallback: similarity {:.2}", sim.similarity_score),
        }
    }
}