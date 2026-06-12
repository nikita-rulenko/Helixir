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
//!   the research doc (P0 + native BM25 hybrid when HelixDB has bm25 enabled).

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

    /// Process-wide cached profile. `from_env` logs on every call — use this
    /// accessor on per-request paths.
    pub fn cached() -> Self {
        static CACHE: std::sync::OnceLock<RetrievalProfile> = std::sync::OnceLock::new();
        *CACHE.get_or_init(Self::from_env)
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

    /// Native HelixDB BM25 + dense vector merged via RRF in Phase 1 (Phase B).
    /// Opt out with `HELIXIR_DISABLE_NATIVE_BM25=1` on an `algo_opt` instance without
    /// BM25 enabled in Helix (graceful fallback to vector-only).
    pub fn native_hybrid_bm25(self) -> bool {
        if !matches!(self, Self::AlgoOpt) {
            return false;
        }
        if std::env::var("HELIXIR_DISABLE_NATIVE_BM25")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        {
            return false;
        }
        true
    }

    /// Cache key must vary with query text when lexical retrieval participates.
    pub fn cache_includes_query_text(self) -> bool {
        matches!(self, Self::AlgoOpt)
    }

    /// Provenance in search results (elder-brain #6): every result's metadata
    /// says whether it was a direct hit (`origin=seed`) or pulled through the
    /// graph (`origin=graph` + edge type, parent memory and depth), so the
    /// agent can see — and verify — the chain instead of a flat list.
    pub fn result_provenance(self) -> bool {
        matches!(self, Self::AlgoOpt)
    }

    /// R3 — reasoning chains walk breadth-first (`VecDeque`) and pick the next
    /// hop by cosine similarity to the query instead of one LLM call per hop.
    /// Removes the last LLM dependency from the read path.
    pub fn embedding_guided_chains(self) -> bool {
        matches!(self, Self::AlgoOpt)
    }

    /// P1.3 — levelwise batched graph expansion: one `getConnectionsLevelBatch`
    /// HQL call per BFS level instead of one `getMemoryLogicalConnections` call
    /// per visited node. Opt out with `HELIXIR_DISABLE_BATCH_EXPANSION=1` if the
    /// query is not deployed on the instance.
    pub fn batched_graph_expansion(self) -> bool {
        if !matches!(self, Self::AlgoOpt) {
            return false;
        }
        if std::env::var("HELIXIR_DISABLE_BATCH_EXPANSION")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        {
            return false;
        }
        true
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
        assert!(!profile.native_hybrid_bm25());
        assert!(!profile.cache_includes_query_text());
        assert!(!profile.batched_graph_expansion());
    }

    #[test]
    fn algo_opt_enables_bundle() {
        let profile = RetrievalProfile::AlgoOpt;
        assert!(profile.real_cosine_for_graph_nodes());
        assert!(profile.temporal_cutoff_in_hql());
        assert!(profile.cache_correctness_fixes());
        assert!(profile.native_hybrid_bm25());
        assert!(profile.cache_includes_query_text());
        assert!(profile.batched_graph_expansion());
        assert_eq!(profile.tag(), "algo_opt");
    }
}
