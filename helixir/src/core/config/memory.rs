//! Memory retrieval, write-path, and maintenance configuration.

use super::*;

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
        Self {
            max: 3,
            initial_delay_ms: 100,
            max_delay_ms: 10_000,
            backoff_factor: 2,
        }
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
        Self {
            implies_in: 0.9,
            because_in: 0.85,
            contradicts_in: 0.8,
            relation_in: 0.6,
        }
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
        Self {
            alpha: 0.6,
            max_iterations: 20,
        }
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
    /// #87: max out-of-window rows graph expansion may return as flagged
    /// flashbacks per search — a separate allowance so associations never
    /// crowd in-window rows.
    pub flashback_max: usize,
    /// #88: at most this many expansion rows get the real-cosine re-rank
    /// per search (top-N by pre-rerank score). Bounds embedding cost on
    /// dense graphs; rows past the cap keep rank-based scores and remain
    /// reachable via PPR.
    pub rerank_max_rows: usize,
    /// #92: score multiplier for rows with an incoming SUPERSEDES edge — a
    /// stale hub must rank below its own corrections, while staying fully
    /// reachable (and honestly flagged `superseded` in metadata). 1.0
    /// disables the demotion.
    pub superseded_penalty: f64,
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
            flashback_max: 3,
            rerank_max_rows: 128,
            superseded_penalty: 0.6,
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
    /// Resolve the preset for a parsed [`crate::core::SearchMode`].
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
    /// #66 follow-up: mint-time synonym convergence + ALIAS_OF wiring. A
    /// candidate (or existing pair of) categories closer than this cosine
    /// are treated as one vocabulary entry — weak models fragment the
    /// dictionary with synonyms, and fragmented subsets blind Lachesis.
    pub alias_threshold: f64,
    /// ALIAS_OF edges wired per pass (bounded like every Moira duty).
    pub alias_max_per_pass: usize,
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
            alias_threshold: 0.86,
            alias_max_per_pass: 4,
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
    /// #83 stitching: how many recent memories one pass scans.
    pub stitch_window: usize,
    /// Max candidate pairs sent to the LLM judge per pass.
    pub stitch_max_judged: usize,
    /// Max BECAUSE edges persisted per pass (the OOM flood lesson).
    pub stitch_max_persist: usize,
    /// Judge confidence below this is discarded.
    pub stitch_min_confidence: u32,
    /// #91: truncate routed threads at a polysemous bridge — an interior
    /// pivot category holding two otherwise-disjoint communities together.
    pub polysemy_guard: bool,
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
            stitch_window: 40,
            stitch_max_judged: 12,
            stitch_max_persist: 6,
            stitch_min_confidence: 70,
            polysemy_guard: true,
        }
    }
}

/// Atropos (the Cutter) curation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AtroposConfig {
    /// #91: hypotheses older than this get an adversarial verification
    /// review (promote / retire / keep). 0 disables the duty.
    pub verify_min_age_hours: f64,
    /// #91: at most this many hypotheses reviewed per verify pass.
    pub verify_max_per_pass: usize,
    /// #91 aged-out policy: a hypothesis with NO witness provenance can never
    /// be verified — past this age it is retired as unverifiable (0 keeps
    /// such hypotheses forever).
    pub verify_unverifiable_age_hours: f64,
    pub quality_pmi_bar: f64,
    pub min_hops: usize,
    pub preference_labels: Vec<String>,
    /// Insight-flood guard: at most this many NEW hypothesis memories persist
    /// per pass. A daemon re-routing a drifting corpus every interval minted
    /// 173 near-duplicate insights in one night without it.
    pub max_persist_per_pass: usize,
}
impl Default for AtroposConfig {
    fn default() -> Self {
        Self {
            verify_min_age_hours: 48.0,
            verify_max_per_pass: 3,
            verify_unverifiable_age_hours: 168.0,
            quality_pmi_bar: 1.0,
            min_hops: 2,
            preference_labels: ["preference", "opinion", "taste", "style", "subjective"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            max_persist_per_pass: 6,
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
    /// Per-pass scan budget for the paraphrase-merge backstop (#43/#55).
    pub merge_limit: i64,
    /// Cosine pre-filter for the merge backstop (the NLI judge is the real gate).
    pub merge_cosine_threshold: f64,
    /// Per-stage cadence: run the stage every Nth daemon pass (1 = every pass,
    /// 0 = never). Lets Clotho tag often while the heavier insight stage
    /// (Lachesis routing + Atropos curation — coupled until insights persist)
    /// runs less frequently.
    pub clotho_every_passes: u64,
    pub insight_every_passes: u64,
    pub merge_every_passes: u64,
    pub reconcile_every_passes: u64,
    /// #83 retroactive causal stitching cadence (0 = never).
    pub stitch_every_passes: u64,
    /// #91 hypothesis verification cadence (0 = never).
    pub verify_every_passes: u64,
}
impl Default for MoiraDaemonConfig {
    fn default() -> Self {
        Self {
            interval_secs: 300,
            reconcile_limit: 500,
            merge_limit: 500,
            merge_cosine_threshold: 0.82,
            clotho_every_passes: 1,
            insight_every_passes: 1,
            stitch_every_passes: 4,
            verify_every_passes: 6,
            merge_every_passes: 1,
            reconcile_every_passes: 1,
        }
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
