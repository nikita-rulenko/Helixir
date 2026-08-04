//! Memory CRUD methods on [`HelixirClient`]: `add`, `add_with_tags`,
//! `search`, `update`, `delete`.

use std::collections::HashMap;

use super::client::HelixirClient;
use super::error::HelixirClientError;
use super::types::{AddMemoryResult, SearchResult, UpdateResult};

/// Client-facing search knobs (#9). Every field is optional — unset means
/// "the configured default" (mode from `default_search_mode`, personal
/// scope, configured limit, no time window).
#[derive(Debug, Clone, Default)]
pub struct SearchParams {
    pub limit: Option<usize>,
    pub search_mode: Option<String>,
    pub temporal_days: Option<f64>,
    pub graph_depth: Option<usize>,
    pub scope: Option<String>,
    pub window: crate::core::TimeWindow,
}

mod read;
mod write;
