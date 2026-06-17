use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchThresholds {
    pub similarity_threshold: f64,
    pub exact_duplicate_score: f64,
    pub min_vector_score: f64,
    pub min_combined_score: f64,
    pub vector_weight: f64,
    pub temporal_weight: f64,
    pub graph_semantic_weight: f64,
    pub graph_graph_weight: f64,
    pub graph_temporal_weight: f64,
    pub default_temporal_days: f64,
    pub bm25_k1: f64,
    pub bm25_b: f64,
}

impl Default for SearchThresholds {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.70,
            exact_duplicate_score: 0.98,
            min_vector_score: 0.5,
            min_combined_score: 0.3,
            vector_weight: 0.7,
            temporal_weight: 0.3,
            graph_semantic_weight: 0.3,
            graph_graph_weight: 0.5,
            graph_temporal_weight: 0.2,
            default_temporal_days: 30.0,
            bm25_k1: 1.5,
            bm25_b: 0.75,
        }
    }
}

/// What Helixir is allowed to do — set explicitly, never inferred. Default is
/// `Solo`: a private memory for one user, with no cross-user behavior and no
/// generative insights. Collective and insights are strict opt-in (HELIXIR_MODE).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryMode {
    /// Personal memory only. No cross-user linking/contradictions; reads stay
    /// personal even if a collective scope is requested. The default.
    Solo,
    /// Shared collective: cross-user linking + contradictions on, collective
    /// reads allowed — but no generative pipeline.
    Collective,
    /// Collective + the generative Moirai (insights, daemon, pipeline).
    Insights,
}

impl MemoryMode {
    /// Lenient parse — anything unrecognized (including empty) falls back to the
    /// safe default, `Solo`. We never silently escalate privilege.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "collective" | "hive" | "shared" => Self::Collective,
            "insights" | "collective+insights" | "full" => Self::Insights,
            _ => Self::Solo,
        }
    }
    /// Cross-user behavior (linking, contradictions, collective reads) allowed.
    pub fn collective_enabled(self) -> bool {
        !matches!(self, Self::Solo)
    }
    /// Generative Moirai (Clotho/Lachesis/Atropos, daemon, pipeline) allowed.
    pub fn insights_enabled(self) -> bool {
        matches!(self, Self::Insights)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Solo => "solo",
            Self::Collective => "collective",
            Self::Insights => "collective+insights",
        }
    }
}

// ── Nested config groups ─────────────────────────────────────────────────────
// Every group derives Serialize + Deserialize and a Default that holds the value
// the code used to hardcode — so wiring a consumer to read config is behavior-
// preserving. A `helixir.toml` may override any subset (the loader merges).

/// Connection retry/backoff (was hardcoded in `db/client.rs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RetryConfig {
    pub max: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff_factor: u64,
}
impl Default for RetryConfig {
    fn default() -> Self {
        Self { max: 3, initial_delay_ms: 100, max_delay_ms: 10_000, backoff_factor: 2 }
    }
}

/// Per-family structural edge weights for graph ranking.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct EdgeWeights {
    pub because: f64,
    pub implies: f64,
    pub similar_to: f64,
    pub memory_relation: f64,
    pub extracted_entity: f64,
    pub contradicts: f64,
    pub default: f64,
}
impl Default for EdgeWeights {
    fn default() -> Self {
        Self {
            because: 1.0,
            implies: 0.9,
            similar_to: 0.75,
            memory_relation: 0.7,
            extracted_entity: 0.6,
            contradicts: 0.4,
            default: 0.5,
        }
    }
}

/// Incoming-edge dampeners (directional reasoning bias).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct EdgeDamping {
    pub implies_in: f64,
    pub because_in: f64,
    pub contradicts_in: f64,
    pub relation_in: f64,
}
impl Default for EdgeDamping {
    fn default() -> Self {
        Self { implies_in: 0.9, because_in: 0.85, contradicts_in: 0.8, relation_in: 0.6 }
    }
}

/// Graph-traversal shape + weights.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphConfig {
    pub depth: usize,
    pub expansion_children_per_parent: usize,
    pub edge_weights: EdgeWeights,
    pub edge_damping: EdgeDamping,
    pub connect_bridge_cap: usize,
    pub connect_bridge_weight: f64,
    pub longest_chain_max_ego_nodes: usize,
    pub longest_chain_max_dfs_steps: usize,
}
impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            depth: 2,
            expansion_children_per_parent: 3,
            edge_weights: EdgeWeights::default(),
            edge_damping: EdgeDamping::default(),
            connect_bridge_cap: 25,
            connect_bridge_weight: 0.5,
            longest_chain_max_ego_nodes: 120,
            longest_chain_max_dfs_steps: 500_000,
        }
    }
}

/// Personalized PageRank re-rank.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PprConfig {
    pub alpha: f64,
    pub max_iterations: usize,
}
impl Default for PprConfig {
    fn default() -> Self {
        Self { alpha: 0.6, max_iterations: 20 }
    }
}

/// Read-path ranking knobs (the dials the bridge-extraction analysis surfaced).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RetrievalConfig {
    pub ppr: PprConfig,
    pub graph: GraphConfig,
    pub rank_base: f64,
    pub rank_decay: f64,
    pub candidate_overfetch: usize,
    pub user_overfetch: usize,
    pub bm25_overfetch: usize,
    pub rerank_min_delta: f64,
    pub collective_user_count_boost: f64,
    pub cross_user_cache_capacity: u64,
    pub cross_user_cache_ttl_secs: u64,
    pub search_modes: SearchModesConfig,
}
impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            ppr: PprConfig::default(),
            graph: GraphConfig::default(),
            rank_base: 0.95,
            rank_decay: 0.92,
            candidate_overfetch: 2,
            user_overfetch: 3,
            bm25_overfetch: 2,
            rerank_min_delta: 0.01,
            collective_user_count_boost: 0.1,
            cross_user_cache_capacity: 1000,
            cross_user_cache_ttl_secs: 60,
            search_modes: SearchModesConfig::default(),
        }
    }
}

/// Per-mode search presets (`recent`/`contextual`/`deep`/`full`). The default
/// values are the canonical match in [`crate::core::search_modes::SearchMode::get_defaults`];
/// this surface makes them TOML/env-overridable. Override a mode by supplying
/// its full block (all fields) — partial per-mode overrides are not merged.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchModesConfig {
    pub recent: crate::core::search_modes::SearchModeDefaults,
    pub contextual: crate::core::search_modes::SearchModeDefaults,
    pub deep: crate::core::search_modes::SearchModeDefaults,
    pub full: crate::core::search_modes::SearchModeDefaults,
}
impl Default for SearchModesConfig {
    fn default() -> Self {
        use crate::core::search_modes::SearchMode;
        Self {
            recent: SearchMode::Recent.get_defaults(),
            contextual: SearchMode::Contextual.get_defaults(),
            deep: SearchMode::Deep.get_defaults(),
            full: SearchMode::Full.get_defaults(),
        }
    }
}
impl SearchModesConfig {
    /// Resolve the preset for a parsed [`SearchMode`].
    #[must_use]
    pub fn for_mode(
        &self,
        mode: crate::core::search_modes::SearchMode,
    ) -> &crate::core::search_modes::SearchModeDefaults {
        use crate::core::search_modes::SearchMode;
        match mode {
            SearchMode::Recent => &self.recent,
            SearchMode::Contextual => &self.contextual,
            SearchMode::Deep => &self.deep,
            SearchMode::Full => &self.full,
        }
    }
}

/// Clotho (the Spinner) policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClothoConfig {
    pub dominance_margin: f64,
    pub grow_threshold: f64,
    pub tag_threshold: f64,
    pub tag_top_k: i64,
    pub mint_confidence: i64,
    pub dict_load_cap: i64,
}
impl Default for ClothoConfig {
    fn default() -> Self {
        Self {
            dominance_margin: 0.07,
            grow_threshold: 0.62,
            tag_threshold: 0.65,
            tag_top_k: 5,
            mint_confidence: 70,
            dict_load_cap: 2000,
        }
    }
}

/// Lachesis (the Measurer) gates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LachesisConfig {
    pub coherence_bar: f64,
    pub min_reasoning_support: f64,
    pub subset_pmi_bar: f64,
    pub dfs_budget: usize,
    pub witnesses_per_hop: usize,
    pub snippet_len: usize,
}
impl Default for LachesisConfig {
    fn default() -> Self {
        Self {
            coherence_bar: 0.5,
            min_reasoning_support: 0.5,
            subset_pmi_bar: 0.5,
            dfs_budget: 200_000,
            witnesses_per_hop: 3,
            snippet_len: 110,
        }
    }
}

/// Atropos (the Cutter) curation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AtroposConfig {
    pub quality_pmi_bar: f64,
    pub min_hops: usize,
    pub preference_labels: Vec<String>,
}
impl Default for AtroposConfig {
    fn default() -> Self {
        Self {
            quality_pmi_bar: 1.0,
            min_hops: 2,
            preference_labels: ["preference", "opinion", "taste", "style", "subjective"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

/// Orchestrator/daemon pass shape (the values clap re-typed inline).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OrchestratorConfig {
    pub corpus_limit: usize,
    pub grow_threshold: f64,
    pub max_seeds: usize,
    pub max_hops: usize,
    pub candidate_cap: i64,
    pub universe_cap: i64,
}
impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            corpus_limit: 500,
            grow_threshold: 0.62,
            max_seeds: 24,
            max_hops: 5,
            candidate_cap: 500,
            universe_cap: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MoiraDaemonConfig {
    pub interval_secs: u64,
    pub reconcile_limit: i64,
}
impl Default for MoiraDaemonConfig {
    fn default() -> Self {
        Self { interval_secs: 300, reconcile_limit: 500 }
    }
}

/// The generative Moirai knobs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MoiraConfig {
    pub clotho: ClothoConfig,
    pub lachesis: LachesisConfig,
    pub atropos: AtroposConfig,
    pub orchestrator: OrchestratorConfig,
    pub daemon: MoiraDaemonConfig,
}

/// Write-path (add pipeline) policy values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WriteConfig {
    pub recall_top_k: usize,
    pub cross_user_dedup_top_k: usize,
    pub cross_user_link_certainty: i64,
    pub cross_user_default_conflict_type: String,
    pub contradict_edge_strength: i64,
    pub entity_link_strength: i64,
    pub entity_link_confidence: i64,
    pub relation_inference_context_k: usize,
    pub raw_source_certainty: u8,
    pub raw_source_importance: u8,
    pub raw_source_min_chars: usize,
    pub fallback_certainty: u8,
    pub fallback_importance: u8,
    pub context_link_priority: i64,
    /// Charter C5: confidence below which a rewrite is escalated to the human.
    pub charter_low_confidence: u8,
}
impl Default for WriteConfig {
    fn default() -> Self {
        Self {
            recall_top_k: 5,
            cross_user_dedup_top_k: 5,
            cross_user_link_certainty: 80,
            cross_user_default_conflict_type: "preference".to_string(),
            contradict_edge_strength: 80,
            entity_link_strength: 80,
            entity_link_confidence: 50,
            relation_inference_context_k: 5,
            raw_source_certainty: 70,
            raw_source_importance: 40,
            raw_source_min_chars: 100,
            fallback_certainty: 50,
            fallback_importance: 50,
            context_link_priority: 50,
            charter_low_confidence: 70,
        }
    }
}

/// Ingest buffer (#25) durability/latency.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IngestConfig {
    pub max_retries: u32,
    pub deadline_secs: u64,
    pub poll_interval_ms: u64,
    pub drain_batch_size: usize,
    pub retry_backoff_ms: u64,
    pub worker_batch_size: usize,
}
impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            deadline_secs: 60,
            poll_interval_ms: 500,
            drain_batch_size: 256,
            retry_backoff_ms: 500,
            worker_batch_size: 32,
        }
    }
}

/// Text chunking (long inputs are split before embedding/storage).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChunkingConfig {
    /// Inputs longer than this many characters are chunked.
    pub threshold: usize,
    /// Target chunk size (characters).
    pub chunk_size: usize,
    /// Embed each chunk on write.
    pub enable_embeddings: bool,
}
impl Default for ChunkingConfig {
    fn default() -> Self {
        Self { threshold: 500, chunk_size: 512, enable_embeddings: true }
    }
}

/// Swarm rendezvous (#39) presence defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SwarmConfig {
    pub active_window_secs: u64,
    pub default_role: String,
    pub default_status: String,
}
impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            active_window_secs: 90,
            default_role: "developer".to_string(),
            default_status: "idle".to_string(),
        }
    }
}

/// Gateway (#42) serving defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    pub default_bind: String,
}
impl Default for GatewayConfig {
    fn default() -> Self {
        Self { default_bind: "0.0.0.0:8765".to_string() }
    }
}

/// LLM/embedding runtime knobs that were previously hardcoded at provider
/// construction (ollama request timeout, embedding cache sizing).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmRuntimeConfig {
    /// Ollama HTTP request timeout (seconds).
    pub request_timeout_secs: u64,
    /// Embedding cache capacity (entries).
    pub embedding_cache_size: usize,
    /// Embedding cache entry TTL (seconds).
    pub embedding_cache_ttl_secs: u64,
}
impl Default for LlmRuntimeConfig {
    fn default() -> Self {
        Self {
            request_timeout_secs: crate::DEFAULT_LLM_REQUEST_TIMEOUT_SECS,
            embedding_cache_size: crate::DEFAULT_CACHE_SIZE,
            embedding_cache_ttl_secs: crate::DEFAULT_CACHE_TTL,
        }
    }
}

/// FastThink (think_* tools) session limits. Defaults match the MCP preset
/// (`FastThinkLimits::mcp`) — the profile the live server runs with.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FastThinkConfig {
    pub max_thoughts: usize,
    pub max_entities: usize,
    pub max_concepts: usize,
    pub max_depth: usize,
    pub thinking_timeout_secs: u64,
    pub session_ttl_secs: u64,
    pub max_recall_results: usize,
}
impl Default for FastThinkConfig {
    fn default() -> Self {
        Self {
            max_thoughts: 150,
            max_entities: 80,
            max_concepts: 40,
            max_depth: 12,
            thinking_timeout_secs: 90,
            session_ttl_secs: 600,
            max_recall_results: 8,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HelixirConfig {
    /// Privilege tier — what the tool is allowed to do (default Solo).
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
    fn config_file_path() -> Option<std::path::PathBuf> {
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
        if let Some(p) = std::env::var("HELIX_PORT").ok().and_then(|p| p.parse().ok()) {
            self.port = p;
        }
        // Privilege tier — opt-in only; unset/unknown stays whatever was set.
        if let Ok(m) = std::env::var("HELIXIR_MODE") {
            self.mode = MemoryMode::parse(&m);
        }
        if let Ok(instance) = std::env::var("HELIX_INSTANCE") {
            self.instance = instance;
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

#[cfg(test)]
mod tests {
    use super::{HelixirConfig, MemoryMode};

    #[test]
    fn test_from_env_reads_llm_base_url() {
        unsafe {
            std::env::set_var("HELIX_LLM_BASE_URL", "http://localhost:11434");
        }

        let config = HelixirConfig::from_env();
        assert_eq!(
            config.llm_base_url.as_deref(),
            Some("http://localhost:11434")
        );

        unsafe {
            std::env::remove_var("HELIX_LLM_BASE_URL");
        }
    }

    #[test]
    fn test_default_has_no_base_url() {
        let config = HelixirConfig::default();
        assert!(config.llm_base_url.is_none());
    }

    #[test]
    fn test_from_env_reads_embedding_url() {
        // Set a recognizable URL different from the ollama default so the
        // assertion catches a regression where embedding_url is shadowed.
        unsafe {
            std::env::set_var("HELIX_EMBEDDING_URL", "https://openrouter.ai/api/v1");
        }

        let config = HelixirConfig::from_env();
        assert_eq!(config.embedding_url, "https://openrouter.ai/api/v1");

        unsafe {
            std::env::remove_var("HELIX_EMBEDDING_URL");
        }
    }

    #[test]
    fn memory_mode_defaults_to_solo_and_never_silently_escalates() {
        assert_eq!(HelixirConfig::default().mode, MemoryMode::Solo);
        assert_eq!(MemoryMode::parse(""), MemoryMode::Solo);
        assert_eq!(MemoryMode::parse("nonsense"), MemoryMode::Solo);
        assert_eq!(MemoryMode::parse("personal"), MemoryMode::Solo);
        assert_eq!(MemoryMode::parse("Collective"), MemoryMode::Collective);
        assert_eq!(MemoryMode::parse("hive"), MemoryMode::Collective);
        assert_eq!(MemoryMode::parse("insights"), MemoryMode::Insights);
        assert_eq!(MemoryMode::parse(" FULL "), MemoryMode::Insights);
    }

    #[test]
    fn partial_toml_overrides_only_named_fields() {
        // A partial file mentions one nested knob; everything else stays default.
        let toml = r#"
            [moira.clotho]
            dominance_margin = 0.99

            [retrieval.ppr]
            alpha = 0.4
        "#;
        let cfg: HelixirConfig = toml::from_str(toml).expect("partial toml parses");
        assert_eq!(cfg.moira.clotho.dominance_margin, 0.99); // overridden
        assert_eq!(cfg.retrieval.ppr.alpha, 0.4); // overridden
        // Untouched fields keep their defaults at every level:
        assert_eq!(cfg.moira.atropos.min_hops, 2);
        assert_eq!(cfg.moira.clotho.grow_threshold, 0.62);
        assert_eq!(cfg.retrieval.ppr.max_iterations, 20);
        assert_eq!(cfg.retrieval.graph.edge_weights.because, 1.0);
        assert_eq!(cfg.host, "localhost");
        assert_eq!(cfg.swarm.active_window_secs, 90);
        assert_eq!(cfg.mode, MemoryMode::Solo);
    }

    #[test]
    fn config_defaults_match_audited_hardcode() {
        let c = HelixirConfig::default();
        assert_eq!(c.retrieval.ppr.alpha, 0.6);
        assert_eq!(c.retrieval.rank_decay, 0.92);
        assert_eq!(c.moira.lachesis.subset_pmi_bar, 0.5);
        assert_eq!(c.moira.atropos.quality_pmi_bar, 1.0);
        assert_eq!(c.write.cross_user_link_certainty, 80);
        assert_eq!(c.ingest.max_retries, 5);
        assert_eq!(c.retry.max, 3);
        assert_eq!(c.gateway.default_bind, "0.0.0.0:8765");
    }

    #[test]
    fn memory_mode_capabilities_are_tiered() {
        assert!(!MemoryMode::Solo.collective_enabled());
        assert!(!MemoryMode::Solo.insights_enabled());
        assert!(MemoryMode::Collective.collective_enabled());
        assert!(!MemoryMode::Collective.insights_enabled());
        assert!(MemoryMode::Insights.collective_enabled());
        assert!(MemoryMode::Insights.insights_enabled());
    }
}
