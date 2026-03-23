mod add_pipeline;
mod crud;
mod events;
mod graph;
pub(crate) mod helpers;
mod reasoning;
mod search;
pub mod types;

pub use types::*;

use std::sync::Arc;

use tracing::{info, warn};

use crate::core::config::HelixirConfig;
use crate::core::events::EventBus;
use crate::db::HelixClient;
use crate::llm::decision::LLMDecisionEngine;
use crate::llm::extractor::LlmExtractor;
use crate::llm::providers::base::LlmProvider;
use crate::llm::EmbeddingGenerator;
use crate::toolkit::mind_toolbox::chunking::ChunkingManager;
use crate::toolkit::mind_toolbox::entity::EntityManager;
use crate::toolkit::mind_toolbox::ontology::OntologyManager;
use crate::toolkit::mind_toolbox::reasoning::ReasoningEngine;
use crate::toolkit::mind_toolbox::search::{SearchEngine, SearchEngineConfig};

pub struct ToolingManager {
    pub(crate) db: Arc<HelixClient>,
    pub(crate) embedder: Arc<EmbeddingGenerator>,
    pub(crate) llm_provider: Arc<dyn LlmProvider>,
    pub(crate) extractor: LlmExtractor<Arc<dyn LlmProvider>>,
    pub(crate) decision_engine: LLMDecisionEngine,
    pub(crate) chunking_manager: ChunkingManager,
    pub(crate) entity_manager: EntityManager,
    pub(crate) ontology_manager: parking_lot::RwLock<OntologyManager>,
    pub(crate) reasoning_engine: ReasoningEngine,
    pub(crate) search_engine: SearchEngine,
    pub(crate) config: HelixirConfig,
    pub(crate) event_bus: Arc<EventBus>,
}

impl ToolingManager {
    pub fn new(
        db: Arc<HelixClient>,
        embedder: Arc<EmbeddingGenerator>,
        llm_provider: Arc<dyn LlmProvider>,
        config: &HelixirConfig,
    ) -> Self {
        info!("ToolingManager initialized with full pipeline");

        let thresholds = &config.search_thresholds;

        let extractor = LlmExtractor::new(Arc::clone(&llm_provider));
        let decision_engine = LLMDecisionEngine::with_thresholds(
            Arc::clone(&llm_provider),
            thresholds.similarity_threshold,
            thresholds.exact_duplicate_score,
        );
        let chunking_manager = ChunkingManager::new(
            Arc::clone(&db),
            Some(Arc::clone(&embedder)),
        );
        let entity_manager = EntityManager::new(Arc::clone(&db), 1000);
        let ontology_manager = parking_lot::RwLock::new(OntologyManager::new(Arc::clone(&db)));
        let reasoning_engine = ReasoningEngine::new(
            Arc::clone(&db),
            Some(Arc::clone(&llm_provider)),
            500,
        );
        let search_engine = SearchEngine::new(
            Arc::clone(&db),
            Arc::clone(&embedder),
            SearchEngineConfig {
                search_thresholds: config.search_thresholds.clone(),
                ..SearchEngineConfig::default()
            },
        );
        let event_bus = Arc::new(EventBus::new());

        Self {
            db,
            embedder,
            llm_provider,
            extractor,
            decision_engine,
            chunking_manager,
            entity_manager,
            ontology_manager,
            reasoning_engine,
            search_engine,
            config: config.clone(),
            event_bus,
        }
    }

    pub async fn initialize(&self) -> Result<(), ToolingError> {
        info!("Initializing ToolingManager - loading ontology");

        let needs_load = {
            let ontology = self.ontology_manager.read();
            !ontology.is_loaded()
        };

        if needs_load {
            let db = Arc::clone(&self.db);
            let mut ontology_manager = OntologyManager::new(db);
            ontology_manager.load().await.map_err(|e| {
                warn!("Failed to load ontology: {}", e);
                ToolingError::from(e)
            })?;

            *self.ontology_manager.write() = ontology_manager;
            info!("Ontology loaded successfully");
        }
        Ok(())
    }
}
