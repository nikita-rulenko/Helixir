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
//! See `helixir/doc/duplication-audit.md` issue #26 for the standing dedup
//! between this `search/` tree and `onto_search/`.

pub mod bm25;
pub mod cache;
pub mod hybrid;
pub mod models;
pub mod onto_search;
pub mod query_processor;
pub mod smart_traversal_v2;
pub mod vector;

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
    SearchConfig as SmartSearchConfig, SmartTraversalV2, calculate_temporal_freshness,
    cosine_similarity, edge_weights,
};

pub use onto_search::{
    OntoSearchConfig, OntoSearchResult, calculate_temporal_freshness as onto_temporal_freshness,
    is_within_temporal_window, parse_datetime_utc,
};

pub use query_processor::{EnhancedQuery, QueryIntent, QueryProcessor};

pub use engine::SearchEngine;
pub use types::{ControversyInfo, SearchEngineConfig, SearchError, UnifiedSearchResult};
