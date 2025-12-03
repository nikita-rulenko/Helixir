pub mod edge_creator;
pub mod finder;
pub mod models;
pub mod reasoner;
pub mod similarity;

use crate::db::HelixClient;
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use tracing::{info, warn};

use self::{
    edge_creator::{EdgeCreator, EdgeCreatorError},
    finder::{FinderError, SimilarMemoryFinder},
    models::{IntegrationConfig, IntegrationResult},
    reasoner::{ReasoningEngine, ReasoningError, RelationInferrer},
};

#[derive(Error, Debug)]
pub enum IntegrationError {
    #[error("Finder error: {0}")]
    Finder(#[from] FinderError),
    #[error("Edge creation error: {0}")]
    EdgeCreation(#[from] EdgeCreatorError),
    #[error("Reasoning error: {0}")]
    Reasoning(#[from] ReasoningError),
    #[error("Integration timeout")]
    Timeout,
}

pub struct MemoryIntegrator {
    finder: SimilarMemoryFinder,
    reasoner: RelationInferrer,
    edge_creator: EdgeCreator,
    config: IntegrationConfig,
}

impl MemoryIntegrator {
    pub fn new(
        client: Arc<HelixClient>,
        config: IntegrationConfig,
        reasoning_engine: Option<Arc<dyn ReasoningEngine>>,
    ) -> Self {
        Self {
            finder: SimilarMemoryFinder::new(client.clone(), config.similarity_threshold, config.max_similar),
            reasoner: RelationInferrer::new(reasoning_engine, config.enable_reasoning),
            edge_creator: EdgeCreator::new(client),
            config,
        }
    }

    pub async fn integrate(
        &self,
        memory_id: &str,
        content: &str,
        embedding: &[f32],
        user_id: &str,
    ) -> Result<IntegrationResult, IntegrationError> {
        let start_time = Instant::now();
        info!("Starting memory integration for {}", memory_id);

        let similar_memories = self
            .finder
            .find_similar(embedding, user_id, Some(memory_id))
            .await?;

        if similar_memories.is_empty() {
            info!("No similar memories found for {}", memory_id);
            return Ok(IntegrationResult {
                memory_id: memory_id.to_string(),
                similar_found: 0,
                relations_created: 0,
                superseded_memories: vec![],
                integration_time_ms: start_time.elapsed().as_millis() as f64,
            });
        }

        let relations = self
            .reasoner
            .infer_relations(content, &similar_memories)
            .await;

        let created_count = self
            .edge_creator
            .create_relations(memory_id, &relations)
            .await?;

        let integration_time_ms = start_time.elapsed().as_millis() as f64;

        info!(
            "Integration complete for {}: {} similar, {} relations created",
            memory_id, similar_memories.len(), created_count
        );

        Ok(IntegrationResult {
            memory_id: memory_id.to_string(),
            similar_found: similar_memories.len(),
            relations_created: created_count,
            superseded_memories: vec![],
            integration_time_ms,
        })
    }
}