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
                .map_err(|e| HelixirClientError::Database(e.to_string()))?
                .with_retry(config.retry.clone()),
        );

        let embedder = Arc::new(EmbeddingGenerator::new(crate::llm::EmbeddingConfig {
            provider: config.embedding_provider.clone(),
            base_url: config.embedding_url.clone(),
            model: config.embedding_model.clone(),
            api_key: config.embedding_api_key.clone(),
            timeout_secs: config.timeout,
            cache_size: config.llm_runtime.embedding_cache_size,
            cache_ttl: config.llm_runtime.embedding_cache_ttl_secs,
            fallback_enabled: config.embedding_fallback_enabled,
            fallback_url: config.embedding_fallback_url.clone(),
            fallback_model: config.embedding_fallback_model.clone(),
        }));

        let primary_llm: Arc<dyn LlmProvider> = LlmProviderFactory::create(
            &config.llm_provider,
            &config.llm_model,
            config.llm_api_key.as_deref(),
            config.llm_base_url.as_deref(),
            f64::from(config.llm_temperature),
            config.llm_runtime.request_timeout_secs,
        )
        .into();

        // Resilience chain: if the primary errors (outage, exhausted quota),
        // the same prompt cascades down llm_fallback_chain — by default
        // smart remote → cheap remote → local Ollama — and readopts the
        // primary as soon as it recovers. Tiers equal to the primary or
        // missing credentials are skipped at boot; with no surviving tier
        // this is an identity passthrough.
        let llm_provider: Arc<dyn LlmProvider> =
            LlmProviderFactory::create_chained(primary_llm, &config);

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

    /// Share the tooling generation with process-owned background services.
    /// Workers must not be spawned from the client itself: hot reload creates
    /// multiple client generations, while the ingest serializer is singular.
    pub(crate) fn tooling_arc(&self) -> Arc<ToolingManager> {
        Arc::clone(&self.tooling_manager)
    }

    /// Clotho the Spinner (#33 / Moira) — the auto-tagging agent over the
    /// category dictionary. Borrows the toolkit it drives.
    pub fn clotho(&self) -> crate::agents::clotho::Clotho<'_> {
        crate::agents::clotho::Clotho::new(self.tooling())
    }

    /// Lachesis the Measurer (#39 / Moira) — routes chains and gates them
    /// against apophenia, labelling survivors as hypotheses-requiring-
    /// verification. Borrows the toolkit it routes over.
    pub fn lachesis(&self) -> crate::agents::lachesis::Lachesis<'_> {
        crate::agents::lachesis::Lachesis::new(self.tooling())
    }

    /// Atropos the Cutter (#48 / Moira) — curates Lachesis threads into ranked,
    /// deduped insights with provenance. Borrows the toolkit.
    pub fn atropos(&self) -> crate::agents::atropos::Atropos<'_> {
        crate::agents::atropos::Atropos::new(self.tooling())
    }

    /// The Moira orchestrator (#41) — runs the full Clotho→Lachesis→Atropos
    /// scenario as one pass. Borrows the toolkit.
    pub fn orchestrator(&self) -> crate::agents::orchestrator::Orchestrator<'_> {
        crate::agents::orchestrator::Orchestrator::new(self.tooling())
    }

    /// The Moira daemon (#42) — schedules orchestrator passes (continuous vs
    /// on-call). Borrows the toolkit.
    pub fn daemon(&self) -> crate::agents::daemon::Daemon<'_> {
        crate::agents::daemon::Daemon::new(self.tooling())
    }
}

impl Drop for HelixirClient {
    fn drop(&mut self) {
        if self.is_initialized.load(Ordering::Relaxed) {
            self.is_initialized.store(false, Ordering::Relaxed);
        }
    }
}
