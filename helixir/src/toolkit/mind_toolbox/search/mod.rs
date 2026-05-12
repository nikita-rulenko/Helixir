//! Search subsystem facade.
//!
//! Public surface:
//! - [`SearchEngine`] — top-level facade, constructed by [`crate::toolkit::tooling_manager`].
//! - [`UnifiedSearchResult`] / [`ControversyInfo`] — result shape consumed by
//!   the MCP layer.
//! - Per-backend types (`VectorSearch`, `HybridSearch`, `Bm25Search`,
//!   `SmartTraversalV2`) re-exported for direct use by tests.
//!
//! Layout:
//! - [`engine`]     — `SearchEngine` struct, constructor, per-backend facades.
//! - [`dispatch`]   — mode-driven [`SearchEngine::search`] + dedup probe.
//! - [`enrichment`] — collective-scope `user_count` / controversy lookups.
//! - [`types`]      — `SearchError`, `SearchEngineConfig`, `UnifiedSearchResult`,
//!   `ControversyInfo`.
//!
//! Resolution of the historical duplication noted in
//! `helixir/doc/duplication-audit.md` (D2 / issue #26): the `onto_search/`
//! tree is the dead twin of `smart_traversal_v2/` — same-name phase
//! functions, parallel result types, never wired into [`SearchEngine`].
//! It is excluded from the live compilation unit below and kept on disk
//! as a historical reference.

pub mod bm25;
pub mod cache;
pub mod hybrid;
pub mod models;
pub mod query_processor;
pub mod smart_traversal_v2;
pub mod vector;

// <unused reason="`onto_search/` is a parallel, never-wired search pipeline (vector_search_phase,
//                graph_expansion_phase, rank_results, classify_query_concepts, etc.) that duplicates
//                the active `smart_traversal_v2/` tree below. No call site outside the module itself.
//                Kept on disk for historical reference and to make a future revival cheap.
//                Closes issue #26 (D2) by removing the duplicate from the live build.
//                See helixir/doc/duplication-audit.md §3.">
// pub mod onto_search;
// </unused>

mod dispatch;
mod engine;
mod enrichment;
mod types;

pub use bm25::Bm25Search;
pub use cache::{CacheStats, SearchCache};
pub use hybrid::{HybridSearch, HybridSearchError};
pub use models::{SearchMethod, SearchResult};
pub use vector::{VectorSearch, VectorSearchError};

pub use smart_traversal_v2::{
    SearchConfig as SmartSearchConfig, SmartTraversalV2, calculate_temporal_freshness, cosine_score,
    edge_weights,
};

pub use query_processor::{EnhancedQuery, QueryIntent, QueryProcessor};

pub use engine::SearchEngine;
pub use types::{ControversyInfo, SearchEngineConfig, SearchError, UnifiedSearchResult};
