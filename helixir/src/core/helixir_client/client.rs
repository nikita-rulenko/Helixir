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

/// Actor-authorized access to Helixir's generative/maintenance internals.
///
/// These APIs operate below the per-memory facade and therefore require a
/// global administrator whenever RBAC is enabled. Keeping the raw accessors
/// crate-private prevents external Rust callers from bypassing that decision.
pub struct HelixirAdmin<'a> {
    client: &'a HelixirClient,
}

impl<'a> HelixirAdmin<'a> {
    pub fn db(&self) -> &'a HelixClient {
        &self.client.db
    }

    pub fn tooling(&self) -> &'a ToolingManager {
        &self.client.tooling_manager
    }

    pub fn clotho(&self) -> crate::agents::clotho::Clotho<'a> {
        crate::agents::clotho::Clotho::new(self.tooling())
    }

    pub fn lachesis(&self) -> crate::agents::lachesis::Lachesis<'a> {
        crate::agents::lachesis::Lachesis::new(self.tooling())
    }

    pub fn atropos(&self) -> crate::agents::atropos::Atropos<'a> {
        crate::agents::atropos::Atropos::new(self.tooling())
    }

    pub fn orchestrator(&self) -> crate::agents::orchestrator::Orchestrator<'a> {
        crate::agents::orchestrator::Orchestrator::new(self.tooling())
    }

    pub fn daemon(&self) -> crate::agents::daemon::Daemon<'a> {
        crate::agents::daemon::Daemon::new(self.tooling())
    }
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

    pub(crate) fn db(&self) -> &HelixClient {
        &self.db
    }

    /// Access the HelixDB-backed RBAC manager used by CLI and MCP paths.
    pub fn rbac(&self) -> crate::core::rbac::RbacManager {
        crate::core::rbac::RbacManager::new(Arc::clone(&self.db))
    }

    pub fn embedder(&self) -> &EmbeddingGenerator {
        &self.embedder
    }

    pub fn llm_provider(&self) -> &dyn LlmProvider {
        &*self.llm_provider
    }

    pub(crate) fn tooling(&self) -> &ToolingManager {
        &self.tooling_manager
    }

    /// Enter the low-level maintenance surface as an authenticated principal.
    pub async fn admin_as(&self, actor_id: &str) -> Result<HelixirAdmin<'_>, HelixirClientError> {
        self.ensure_initialized().await?;
        self.rbac()
            .authorize_admin_surface(actor_id)
            .await
            .map_err(|e| HelixirClientError::Operation(e.to_string()))?;
        Ok(HelixirAdmin { client: self })
    }

    /// Share the tooling generation with process-owned background services.
    /// Workers must not be spawned from the client itself: hot reload creates
    /// multiple client generations, while the ingest serializer is singular.
    pub(crate) fn tooling_arc(&self) -> Arc<ToolingManager> {
        Arc::clone(&self.tooling_manager)
    }
}

impl Drop for HelixirClient {
    fn drop(&mut self) {
        if self.is_initialized.load(Ordering::Relaxed) {
            self.is_initialized.store(false, Ordering::Relaxed);
        }
    }
}
