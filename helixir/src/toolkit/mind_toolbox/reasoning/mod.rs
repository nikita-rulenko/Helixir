//! Reasoning subsystem: typed `BECAUSE / IMPLIES / SUPPORTS / CONTRADICTS`
//! edges on top of `Memory` nodes, plus BFS chain traversal and LLM-driven
//! inference of relations between similar memories.
//!
//! Layout:
//! - [`types`]  — pure data types ([`ReasoningType`], [`ReasoningRelation`],
//!   [`ReasoningChain`], [`CacheStats`], [`ReasoningError`]) plus the
//!   `project_relation` projection helper.
//! - [`engine`] — [`ReasoningEngine`] struct, constructor, cache management,
//!   `build_reasoning_trail` formatter.
//! - [`edges`]  — `add_relation` + `edge_exists` (CRUD on reasoning edges).
//! - [`chain`]  — `get_chain` BFS traversal across the 8 logical directions.
//! - [`infer`]  — `infer_relations` (LLM-guided relation extraction).
//!
//! Public surface kept identical to the pre-split `engine.rs`.

mod chain;
mod edges;
mod engine;
mod infer;
mod types;

pub use chain::ChainGuidance;
pub use engine::ReasoningEngine;
pub use types::{CacheStats, ReasoningChain, ReasoningError, ReasoningRelation, ReasoningType};
