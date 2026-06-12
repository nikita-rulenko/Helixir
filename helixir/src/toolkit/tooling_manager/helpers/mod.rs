//! Small `impl ToolingManager` helpers, split by concern.
//!
//! Each submodule extends [`super::ToolingManager`] with a focused group of
//! private methods. The split avoids a 500-line helpers.rs without changing
//! any public API.
//!
//! - [`queries`]  — read/write helpers against `Memory` nodes
//!   (`get_memory_type`, `update_memory_internal`).
//! - [`users`]    — user lifecycle and Hive cross-user linking.
//! - [`concepts`] — memory ↔ concept linking on the live add path.
//! - [`history`]  — `HistoryEvent` writes for the audit trail.
//! - [`reserved`] — wrappers around helix queries that exist DB-side but are
//!   not yet wired into the active pipelines. Kept here so the API surface
//!   stays type-checked even while unused.
//!
//! The canonical `safe_truncate` is `crate::safe_truncate` (defined in
//! `helixir/src/utils.rs`, re-exported from `lib.rs`). Issue #27 (D3
//! duplication) is closed by deleting the local copy that used to sit here.

mod concepts;
mod history;
mod queries;
mod reserved;
mod users;
