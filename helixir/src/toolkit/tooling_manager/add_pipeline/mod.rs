//! `add_memory` write pipeline — the largest single responsibility on
//! [`super::ToolingManager`]. The previous monolithic 1.5k-LOC file is split
//! into focused modules; each module is a `impl ToolingManager` extension.
//!
//! Top-level layout:
//! - [`orchestrate`] — the `pub async fn add_memory` entry point.
//! - [`prepare`]     — extracted-memory hygiene (`is_coherent_memory`,
//!   splitting, count-of-subjects heuristic).
//! - [`recall`]      — reserved personal-only embedding+search probe
//!   (`embed_and_search_personal`).
//! - [`decide`]      — Phase 1: `handle_memory_operation` that maps a
//!   [`crate::llm::decision::MemoryDecision`] to ADD/UPDATE/SUPERSEDE/CONTRADICT/DELETE/NOOP.
//! - [`cross_user`]  — Phase 2: `apply_cross_user_phase` + background
//!   `link_user_to_memory_bg` / `add_contradiction_bg` helpers.
//! - [`enrich`]      — relation inference, entity linking, concept linking;
//!   `enrich_memory_relations` + `resolve_and_persist_extraction_relations`.
//! - [`store`]       — `store_new_memory` (live ADD) and `store_raw_source`
//!   (long-input source preservation).
//! - [`entity_links`] — `persist_entity_relation` for entity → entity edges.
//! - [`context_link`] — `link_memory_to_extracted_context` (creates the
//!   `Context` node on miss).
//!
//! See `helixir/doc/dataflow.md` for the end-to-end picture and `AGENTS.md`
//! §1bis for the load-bearing invariants this pipeline preserves.

mod connective_backstop;
mod context_link;
mod cross_user;
mod decide;
mod enrich;
mod entity_links;
mod orchestrate;
mod prepare;
mod recall;
pub(crate) mod store;
