//! Root configuration object and environment resolution.

use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HelixirConfig {
    /// Memory mode — which collaboration capabilities are enabled (default Solo).
    pub mode: MemoryMode,
    pub host: String,
    pub port: u16,
    pub instance: String,
    pub api_key: Option<String>,
    pub timeout: u64,
    pub max_retries: u32,

    pub llm_provider: String,
    pub llm_model: String,
    pub llm_api_key: Option<String>,
    pub llm_base_url: Option<String>,
    pub llm_temperature: f32,

    pub llm_fallback_enabled: bool,
    pub llm_fallback_url: String,
    pub llm_fallback_model: String,
    /// Ordered provider names tried after the primary (smart → cheap → local).
    /// Entries equal to the primary, unknown names, or tiers missing
    /// credentials are skipped at boot with a warning — a partial chain still
    /// boots. Env: `HELIX_LLM_FALLBACK_CHAIN` (comma-separated).
    pub llm_fallback_chain: Vec<String>,
    /// Credentials for the `deepseek` chain tier (the primary's key lives in
    /// `llm_api_key`). Env: `HELIX_DEEPSEEK_API_KEY` / `HELIX_DEEPSEEK_MODEL`.
    pub deepseek_api_key: Option<String>,
    pub deepseek_model: String,

    pub embedding_provider: String,
    pub embedding_model: String,
    pub embedding_url: String,
    pub embedding_api_key: Option<String>,

    pub embedding_fallback_enabled: bool,
    pub embedding_fallback_url: String,
    pub embedding_fallback_model: String,

    pub default_certainty: u8,
    pub default_importance: u8,

    pub default_search_limit: usize,
    pub default_search_mode: String,
    /// #64: when a personal-scope recall returns fewer than this many hits and
    /// the collective tier is enabled, search_memory appends a hint nudging the
    /// agent to retry with scope=collective. 0 disables the hint.
    pub recall_thin_hint_threshold: usize,
    pub vector_search_enabled: bool,
    pub graph_search_enabled: bool,
    pub bm25_search_enabled: bool,

    pub search_thresholds: SearchThresholds,

    pub max_facts_per_call: usize,

    /// Entity-resolution LRU cache capacity (EntityManager).
    pub entity_cache_size: usize,
    /// Max memories pulled as context when reconstructing reasoning chains.
    pub reasoning_context_limit: usize,

    // Nested groups (externalized hardcode). Serde-default so a partial
    // helixir.toml need only mention what it overrides.
    #[serde(default)]
    pub retry: RetryConfig,
    #[serde(default)]
    pub retrieval: RetrievalConfig,
    #[serde(default)]
    pub moira: MoiraConfig,
    pub watchdog: WatchdogConfig,
    #[serde(default)]
    pub write: WriteConfig,
    #[serde(default)]
    pub ingest: IngestConfig,
    #[serde(default)]
    pub chunking: ChunkingConfig,
    #[serde(default)]
    pub swarm: SwarmConfig,
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub llm_runtime: LlmRuntimeConfig,
    #[serde(default)]
    pub fast_think: FastThinkConfig,
}

impl HelixirConfig {
    pub fn new(host: &str, port: u16) -> Self {
        Self {
            mode: MemoryMode::Solo,
            host: host.to_string(),
            port,
            instance: "dev".to_string(),
            api_key: None,
            timeout: 30,
            max_retries: 3,

            llm_provider: "cerebras".to_string(),
            llm_model: crate::DEFAULT_LLM_MODEL.to_string(),
            llm_api_key: None,
            llm_base_url: None,
            llm_temperature: 0.3,

            llm_fallback_enabled: true,
            llm_fallback_url: crate::DEFAULT_OLLAMA_URL.to_string(),
            llm_fallback_model: crate::DEFAULT_LLM_FALLBACK_MODEL.to_string(),
            llm_fallback_chain: vec!["deepseek".to_string(), "ollama".to_string()],
            deepseek_api_key: None,
            deepseek_model: crate::DEFAULT_DEEPSEEK_MODEL.to_string(),

            embedding_provider: "ollama".to_string(),
            embedding_model: crate::DEFAULT_EMBEDDING_MODEL.to_string(),
            embedding_url: crate::DEFAULT_OLLAMA_URL.to_string(),
            embedding_api_key: None,

            embedding_fallback_enabled: true,
            embedding_fallback_url: crate::DEFAULT_OLLAMA_URL.to_string(),
            embedding_fallback_model: crate::DEFAULT_EMBEDDING_MODEL.to_string(),

            default_certainty: 80,
            default_importance: 50,

            default_search_limit: 10,
            default_search_mode: "recent".to_string(),
            recall_thin_hint_threshold: 3,
            vector_search_enabled: true,
            graph_search_enabled: true,
            bm25_search_enabled: true,

            search_thresholds: SearchThresholds::default(),

            max_facts_per_call: 15,
            entity_cache_size: 1000,
            reasoning_context_limit: 500,

            retry: RetryConfig::default(),
            retrieval: RetrievalConfig::default(),
            moira: MoiraConfig::default(),
            watchdog: WatchdogConfig::default(),
            write: WriteConfig::default(),
            ingest: IngestConfig::default(),
            chunking: ChunkingConfig::default(),
            swarm: SwarmConfig::default(),
            gateway: GatewayConfig::default(),
            llm_runtime: LlmRuntimeConfig::default(),
            fast_think: FastThinkConfig::default(),
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    /// Build the embedding client configuration shared by runtime writes and
    /// one-way graph migrations that create first-class Memory nodes.
    pub fn embedding_config(&self) -> crate::llm::EmbeddingConfig {
        crate::llm::EmbeddingConfig {
            provider: self.embedding_provider.clone(),
            base_url: self.embedding_url.clone(),
            model: self.embedding_model.clone(),
            api_key: self.embedding_api_key.clone(),
            timeout_secs: self.timeout,
            cache_size: self.llm_runtime.embedding_cache_size,
            cache_ttl: self.llm_runtime.embedding_cache_ttl_secs,
            fallback_enabled: self.embedding_fallback_enabled,
            fallback_url: self.embedding_fallback_url.clone(),
            fallback_model: self.embedding_fallback_model.clone(),
        }
    }

    /// The public entry point. Layered: built-in defaults → `helixir.toml`
    /// (if present) → `HELIX_*`/`HELIXIR_*` env (env wins). All existing callers
    /// (MCP server, gateway, CLI, client) reach the layered config through this.
    pub fn from_env() -> Self {
        Self::load()
    }

    /// defaults → helixir.toml → env.
    pub fn load() -> Self {
        let mut config = Self::from_toml_file().unwrap_or_default();
        config.overlay_env();
        config
    }

    /// Resolve the optional config file: `$HELIXIR_CONFIG`, else
    /// `~/.helixir/helixir.toml`, else `./helixir.toml`. Returns the first that
    /// exists.
    pub fn config_file_path() -> Option<std::path::PathBuf> {
        if let Ok(p) = std::env::var("HELIXIR_CONFIG") {
            let p = std::path::PathBuf::from(p);
            return p.exists().then_some(p);
        }
        if let Ok(home) = std::env::var("HOME") {
            let p = std::path::PathBuf::from(home).join(".helixir/helixir.toml");
            if p.exists() {
                return Some(p);
            }
        }
        let cwd = std::path::PathBuf::from("helixir.toml");
        cwd.exists().then_some(cwd)
    }

    /// Merge a `helixir.toml` over the built-in defaults. Every struct is
    /// `#[serde(default)]`, so a partial file need only mention what it
    /// overrides — missing fields fall back to `Default`. `None` when no file is
    /// found; logs and falls back to defaults on a malformed file.
    fn from_toml_file() -> Option<Self> {
        let path = Self::config_file_path()?;
        let content = std::fs::read_to_string(&path).ok()?;
        match toml::from_str::<Self>(&content) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                eprintln!(
                    "helixir: ignoring malformed {} ({e}); using defaults",
                    path.display()
                );
                Some(Self::default())
            }
        }
    }

    /// Overlay `HELIX_*`/`HELIXIR_*` env onto an existing config (env wins).
    fn overlay_env(&mut self) {
        if let Ok(v) = std::env::var("HELIX_HOST") {
            self.host = v;
        }
        if let Some(p) = std::env::var("HELIX_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
        {
            self.port = p;
        }
        // Memory mode — opt-in only; unset/unknown stays whatever was set.
        if let Ok(m) = std::env::var("HELIXIR_MODE") {
            self.mode = MemoryMode::parse(&m);
        }
        if let Ok(instance) = std::env::var("HELIX_INSTANCE") {
            self.instance = instance;
        }
        if let Ok(token) = std::env::var("HELIXIR_GATEWAY_TOKEN") {
            self.gateway.auth_token = (!token.is_empty()).then_some(token);
        }
        if let Ok(provider) = std::env::var("HELIX_LLM_PROVIDER") {
            self.llm_provider = provider;
        }
        if let Ok(model) = std::env::var("HELIX_LLM_MODEL") {
            self.llm_model = model;
        }
        if let Ok(key) = std::env::var("HELIX_LLM_API_KEY") {
            self.llm_api_key = Some(key);
        }
        if let Ok(url) = std::env::var("HELIX_LLM_BASE_URL") {
            self.llm_base_url = Some(url);
        }
        // Comma-separated tier names; an explicitly empty value clears the
        // chain (fallback off without touching llm_fallback_enabled).
        if let Ok(chain) = std::env::var("HELIX_LLM_FALLBACK_CHAIN") {
            self.llm_fallback_chain = chain
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
        }
        if let Ok(key) = std::env::var("HELIX_DEEPSEEK_API_KEY") {
            self.deepseek_api_key = Some(key);
        }
        if let Ok(model) = std::env::var("HELIX_DEEPSEEK_MODEL") {
            self.deepseek_model = model;
        }
        if let Ok(provider) = std::env::var("HELIX_EMBEDDING_PROVIDER") {
            self.embedding_provider = provider;
        }
        if let Ok(model) = std::env::var("HELIX_EMBEDDING_MODEL") {
            self.embedding_model = model;
        }
        if let Ok(url) = std::env::var("HELIX_EMBEDDING_URL") {
            self.embedding_url = url;
        }
        if let Ok(key) = std::env::var("HELIX_EMBEDDING_API_KEY") {
            self.embedding_api_key = Some(key);
        }
        if let Some(n) = std::env::var("HELIX_MAX_FACTS_PER_CALL")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
        {
            self.max_facts_per_call = n;
        }
    }
}

impl Default for HelixirConfig {
    fn default() -> Self {
        Self::new("localhost", crate::DEFAULT_HELIX_PORT)
    }
}
