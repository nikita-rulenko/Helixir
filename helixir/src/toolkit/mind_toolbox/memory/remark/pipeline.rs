

use std::sync::Arc;
use tracing::info;
use crate::db::HelixClient;
use crate::llm::extractor::LlmExtractor;
use crate::toolkit::mind_toolbox::entity::EntityManager;
use crate::toolkit::mind_toolbox::ontology::OntologyManager;
use super::models::{RemarkStats, UnmarkedMemory};
use super::batch::{get_unmarked_memories, remark_batch, remark_all_unmarked};


pub struct ReMarkupPipeline {
    db_client: Arc<HelixClient>,
    llm_extractor: Arc<LlmExtractor>,
    entity_manager: Arc<EntityManager>,
    ontology_manager: Arc<OntologyManager>,
}

impl ReMarkupPipeline {
    
    pub fn new(
        db_client: Arc<HelixClient>,
        llm_extractor: Arc<LlmExtractor>,
        entity_manager: Arc<EntityManager>,
        ontology_manager: Arc<OntologyManager>,
    ) -> Self {
        info!("ReMarkupPipeline initialized");
        Self {
            db_client,
            llm_extractor,
            entity_manager,
            ontology_manager,
        }
    }

    
    pub async fn get_unmarked(&self, user_id: &str, limit: usize) -> Result<Vec<UnmarkedMemory>, String> {
        get_unmarked_memories(&self.db_client, user_id, limit).await
    }

    
    pub async fn remark_batch(&self, memories: Vec<UnmarkedMemory>, batch_size: usize) -> RemarkStats {
        remark_batch(
            &self.db_client,
            &self.llm_extractor,
            &self.entity_manager,
            &self.ontology_manager,
            memories,
            batch_size,
        ).await
    }

    
    pub async fn remark_all(&self, user_id: &str, batch_size: usize) -> Result<RemarkStats, String> {
        remark_all_unmarked(
            &self.db_client,
            &self.llm_extractor,
            &self.entity_manager,
            &self.ontology_manager,
            user_id,
            batch_size,
        ).await
    }
}
