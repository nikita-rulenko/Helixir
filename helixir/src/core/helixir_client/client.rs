//! [`HelixirClient`] struct, constructor, lifecycle and accessors.
//!
//! Feature methods (memory/graph/concept) live in sibling modules as
//! additional `impl HelixirClient` blocks.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tracing::info;

use crate::core::config::HelixirConfig;
use crate::db::HelixClient;
use crate::llm::EmbeddingGenerator;
use crate::llm::factory::LlmProviderFactory;
use crate::llm::providers::base::LlmProvider;
use crate::toolkit::tooling_manager::ToolingManager;

use super::error::HelixirClientError;

pub struct HelixirClient {
    pub(super) config: HelixirConfig,
    pub(super) db: Arc<HelixClient>,
    pub(super) embedder: Arc<EmbeddingGenerator>,
    pub(super) llm_provider: Arc<dyn LlmProvider>,
    pub(super) tooling_manager: Arc<ToolingManager>,
    pub(super) is_initialized: Arc<AtomicBool>,
}

impl HelixirClient {
    pub fn new(config: HelixirConfig) -> Result<Self, HelixirClientError> {
        let db = Arc::new(
            HelixClient::new(&config.host, config.port)
                .map_err(|e| HelixirClientError::Database(e.to_string()))?,
        );

        let embedder = Arc::new(EmbeddingGenerator::new(crate::llm::EmbeddingConfig {
            provider: config.embedding_provider.clone(),
            base_url: config.embedding_url.clone(),
            model: config.embedding_model.clone(),
            api_key: config.embedding_api_key.clone(),
            timeout_secs: config.timeout,
            cache_size: 1000,
            cache_ttl: 300,
            fallback_enabled: config.embedding_fallback_enabled,
            fallback_url: config.embedding_fallback_url.clone(),
            fallback_model: config.embedding_fallback_model.clone(),
        }));

        let llm_provider: Arc<dyn LlmProvider> = LlmProviderFactory::create(
            &config.llm_provider,
            &config.llm_model,
            config.llm_api_key.as_deref(),
            config.llm_base_url.as_deref(),
            f64::from(config.llm_temperature),
        )
        .into();

        let tooling_manager = Arc::new(ToolingManager::new(
            Arc::clone(&db),
            Arc::clone(&embedder),
            Arc::clone(&llm_provider),
            &config,
        ));

        info!("HelixirClient created with ToolingManager");

        Ok(Self {
            config,
            db,
            embedder,
            llm_provider,
            tooling_manager,
            is_initialized: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn from_env() -> Result<Self, HelixirClientError> {
        let config = HelixirConfig::from_env();
        Self::new(config)
    }

    pub async fn initialize(&self) -> Result<(), HelixirClientError> {
        if self.is_initialized.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.db
            .health_check()
            .await
            .map_err(|e| HelixirClientError::Database(e.to_string()))?;

        self.tooling_manager
            .initialize()
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        // Ingest buffer (#25): one serial background worker drains the queue
        // when HELIXIR_INGEST_BUFFER=1. The synchronous path stays the default.
        if crate::toolkit::tooling_manager::ingest_buffer::buffer_enabled() {
            let tm = Arc::clone(&self.tooling_manager);
            tokio::spawn(crate::toolkit::tooling_manager::ingest_buffer::run_ingest_worker(tm));
        }

        self.is_initialized.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub async fn close(&self) -> Result<(), HelixirClientError> {
        if !self.is_initialized.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.is_initialized.store(false, Ordering::Relaxed);
        Ok(())
    }

    pub(super) async fn ensure_initialized(&self) -> Result<(), HelixirClientError> {
        if !self.is_initialized.load(Ordering::Relaxed) {
            self.initialize().await?;
        }
        Ok(())
    }

    pub fn config(&self) -> &HelixirConfig {
        &self.config
    }

    pub fn db(&self) -> &HelixClient {
        &self.db
    }

    pub fn embedder(&self) -> &EmbeddingGenerator {
        &self.embedder
    }

    pub fn llm_provider(&self) -> &dyn LlmProvider {
        &*self.llm_provider
    }

    pub fn tooling(&self) -> &ToolingManager {
        &self.tooling_manager
    }

    /// Clotho the Spinner (#33 / Moira) — the auto-tagging agent over the
    /// category dictionary. Borrows the toolkit it drives.
    pub fn clotho(&self) -> crate::agents::clotho::Clotho<'_> {
        crate::agents::clotho::Clotho::new(self.tooling())
    }
}

impl Drop for HelixirClient {
    fn drop(&mut self) {
        if self.is_initialized.load(Ordering::Relaxed) {
            self.is_initialized.store(false, Ordering::Relaxed);
        }
    }
}
