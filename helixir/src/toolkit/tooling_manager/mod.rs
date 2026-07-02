mod add_pipeline;
pub mod categories;
pub mod consolidate;
pub mod content_key;
pub mod contradictions;
mod crud;
mod events;
mod graph;
pub(crate) mod helpers;
pub mod ingest_buffer;
mod reasoning;
mod search;
pub mod seeds;
pub mod swarm;
pub mod types;

pub use types::*;

use std::sync::Arc;

use tracing::{info, warn};

use crate::core::config::HelixirConfig;
use crate::core::events::EventBus;
use crate::db::HelixClient;
use crate::llm::EmbeddingGenerator;
use crate::llm::decision::LLMDecisionEngine;
use crate::llm::extractor::LlmExtractor;
use crate::llm::providers::base::LlmProvider;
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
    pub(crate) decision_engine: Arc<LLMDecisionEngine>,
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
        let chunking_manager = ChunkingManager::with_config(
            Arc::clone(&db),
            Some(Arc::clone(&embedder)),
            config.chunking.threshold,
            config.chunking.chunk_size,
            config.chunking.enable_embeddings,
        );
        let entity_manager = EntityManager::new(Arc::clone(&db), config.entity_cache_size);
        let ontology_manager = parking_lot::RwLock::new(OntologyManager::new(Arc::clone(&db)));
        let reasoning_engine = ReasoningEngine::new(
            Arc::clone(&db),
            Some(Arc::clone(&llm_provider)),
            config.reasoning_context_limit,
        );
        let search_engine = SearchEngine::new(
            Arc::clone(&db),
            Arc::clone(&embedder),
            SearchEngineConfig {
                search_thresholds: config.search_thresholds.clone(),
                retrieval: config.retrieval.clone(),
                ..SearchEngineConfig::default()
            },
        );
        let event_bus = Arc::new(EventBus::new());

        Self {
            db,
            embedder,
            llm_provider,
            extractor,
            decision_engine: Arc::new(decision_engine),
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

        self.verify_algo_opt_deployment().await;
        self.maybe_warm_embed_cache().await;
        self.maybe_seed_system_memories().await;
        Ok(())
    }

    /// algo-opt R2: optionally pre-embed the whole corpus so re-rank phases
    /// never pay an ollama round-trip at query time. Opt in with
    /// `HELIXIR_EMBED_CACHE_WARMUP=1` (background) or `=blocking` (await before
    /// serving — for short-lived processes like benches). Pairs with
    /// `HELIXIR_EMBED_CACHE_PATH`, which persists the warmed entries across
    /// restarts — after the first warm run the startup cost is a file read.
    async fn maybe_warm_embed_cache(&self) {
        let mode = std::env::var("HELIXIR_EMBED_CACHE_WARMUP").unwrap_or_default();
        let mode = mode.trim().to_ascii_lowercase();
        if mode.is_empty() || mode == "0" || mode == "false" {
            return;
        }

        let db = Arc::clone(&self.db);
        let embedder = Arc::clone(&self.embedder);
        let task = warm_embed_cache(db, embedder);
        if mode == "blocking" {
            task.await;
        } else {
            tokio::spawn(task);
        }
    }
    /// Upgrade guard (#21): under `algo_opt` the read path needs HQL queries
    /// that older deployments don't have. Each call site falls back
    /// gracefully, but silently — this startup probe turns a misdeployed
    /// instance into one loud, actionable warning instead.
    async fn verify_algo_opt_deployment(&self) {
        let profile = crate::core::RetrievalProfile::cached();
        if !matches!(profile, crate::core::RetrievalProfile::AlgoOpt) {
            return;
        }

        let mut missing: Vec<&str> = Vec::new();

        let probe = serde_json::json!({ "memory_ids": ["helixir-startup-probe"] });
        if self
            .db
            .execute_query::<serde_json::Value, _>("getConnectionsLevelBatch", &probe)
            .await
            .is_err()
        {
            missing.push("getConnectionsLevelBatch (batched graph expansion)");
        }

        let probe = serde_json::json!({ "text": "helixir-startup-probe", "limit": 1 });
        match self
            .db
            .execute_query::<serde_json::Value, _>("searchMemoriesByBm25", &probe)
            .await
        {
            Err(_) => missing.push("searchMemoriesByBm25 (BM25 hybrid)"),
            Ok(_) => {}
        }

        // smartVectorSearchWithChunksCutoff ships in the same schema as the
        // two queries above — probing it would need a correctly-dimensioned
        // vector, so the two probes above stand in for the whole deployment.

        if missing.is_empty() {
            info!("algo_opt deployment check: all required HQL queries present");
        } else {
            warn!(
                "algo_opt is active but this HelixDB instance is missing: {}. \
                 Searches will silently fall back to slower/legacy paths. \
                 Fix: deploy the current schema (make deploy-schema / helix push) \
                 and ensure bm25 = true in the instance config — see UPGRADING.md.",
                missing.join("; ")
            );
        }
    }
}

/// Fetch every memory's content and run it through the (persistent) embedding
/// cache. See [`ToolingManager::maybe_warm_embed_cache`].
async fn warm_embed_cache(db: Arc<HelixClient>, embedder: Arc<EmbeddingGenerator>) {
    #[derive(serde::Deserialize)]
    struct MemoriesResponse {
        #[serde(default)]
        memories: Vec<MemoryContent>,
    }
    #[derive(serde::Deserialize)]
    struct MemoryContent {
        #[serde(default)]
        content: String,
    }

    let params = serde_json::json!({ "limit": 100_000 });
    let response: MemoriesResponse = match db.execute_query("getRecentMemories", &params).await {
        Ok(r) => r,
        Err(e) => {
            warn!("Embed cache warmup: corpus fetch failed: {}", e);
            return;
        }
    };

    let contents: Vec<&str> = response
        .memories
        .iter()
        .map(|m| m.content.as_str())
        .filter(|c| !c.is_empty())
        .collect();
    let total = contents.len();
    let started = std::time::Instant::now();
    for chunk in contents.chunks(64) {
        if let Err(e) = embedder.generate_batch(chunk, true).await {
            warn!("Embed cache warmup: batch failed: {}", e);
            return;
        }
    }
    info!(
        "Embed cache warmup: {} memories embedded in {}ms",
        total,
        started.elapsed().as_millis()
    );
}
