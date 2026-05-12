//! Retrieval pipeline profile selector.
//!
//! Switches between the legacy `SmartTraversalV2` behaviour and the
//! `algo-opt` proposals described in `helixir/doc/retrieval-research.md`
//! without breaking the legacy path. Selected at process start from the
//! `HELIXIR_RETRIEVAL_PROFILE` environment variable.
//!
//! Recognised values (case-insensitive):
//! - `legacy` (default) — preserves current behaviour bit-for-bit.
//! - `algo_opt` / `algo-opt` / `opt` — enables the bundle of fixes from
//!   §6 P0 of the research doc.

use tracing::info;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RetrievalProfile {
    #[default]
    Legacy,
    AlgoOpt,
}

impl RetrievalProfile {
    pub fn from_env() -> Self {
        let raw = std::env::var("HELIXIR_RETRIEVAL_PROFILE").ok();
        let normalized = raw
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase);

        let profile = match normalized.as_deref() {
            Some("algo_opt") | Some("algo-opt") | Some("opt") => Self::AlgoOpt,
            _ => Self::Legacy,
        };

        info!(profile = ?profile, "Retrieval profile selected");
        profile
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::AlgoOpt => "algo_opt",
        }
    }

    /// P0.2 — re-embed graph neighbours and score them with real cosine
    /// instead of the hard-coded `semantic_sim = 0.5` baseline.
    pub fn real_cosine_for_graph_nodes(self) -> bool {
        matches!(self, Self::AlgoOpt)
    }

    /// P0.1 — push `temporal_cutoff` into the HQL `::WHERE` post-filter
    /// (via `smartVectorSearchWithChunksCutoff`) instead of dropping
    /// candidates in Rust after ANN returns.
    pub fn temporal_cutoff_in_hql(self) -> bool {
        matches!(self, Self::AlgoOpt)
    }

    /// P0.3 + P0.4 — include `temporal_cutoff` and the profile tag in the
    /// LRU cache key, and respect `cache_ttl` on entries.
    pub fn cache_correctness_fixes(self) -> bool {
        matches!(self, Self::AlgoOpt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_legacy() {
        let profile = RetrievalProfile::default();
        assert_eq!(profile, RetrievalProfile::Legacy);
        assert!(!profile.real_cosine_for_graph_nodes());
        assert!(!profile.temporal_cutoff_in_hql());
        assert!(!profile.cache_correctness_fixes());
    }

    #[test]
    fn algo_opt_enables_all_p0_fixes() {
        let profile = RetrievalProfile::AlgoOpt;
        assert!(profile.real_cosine_for_graph_nodes());
        assert!(profile.temporal_cutoff_in_hql());
        assert!(profile.cache_correctness_fixes());
        assert_eq!(profile.tag(), "algo_opt");
    }
}
