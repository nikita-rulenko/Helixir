//! Top-level dispatch for [`super::SearchEngine`]:
//! [`SearchEngine::search`] (mode-driven user query) and
//! [`SearchEngine::search_for_dedup`] (lightweight cross-user dedup probe).

use std::sync::Arc;

use serde_json::json;
use tracing::{debug, info};

use crate::core::TimeWindow;
use crate::core::search_modes::SearchMode;

use super::engine::{SearchEngine, embedding_cache_key};
use super::types::{SearchError, UnifiedSearchResult};

/// #87: split one deduped, score-ordered result stream into the honest
/// window (`limit` in-window rows) plus the flashback allowance (at most
/// `flashback_max` out-of-window rows the graph pulled back in, appended
/// AFTER the in-window rows so they never crowd them out).
fn clamp_with_flashbacks(
    results: Vec<UnifiedSearchResult>,
    limit: usize,
    flashback_max: usize,
) -> Vec<UnifiedSearchResult> {
    let mut seen = std::collections::HashSet::new();
    let (flashbacks, in_window): (Vec<_>, Vec<_>) = results
        .into_iter()
        .filter(|r| seen.insert(r.memory_id.clone()))
        .partition(|r| {
            r.metadata
                .get("flashback")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        });
    let mut out: Vec<UnifiedSearchResult> = in_window.into_iter().take(limit).collect();
    out.extend(flashbacks.into_iter().take(flashback_max));
    out
}

mod projection;
mod search;

fn superseded_window(limit: usize, result_count: usize) -> usize {
    limit
        .saturating_mul(3)
        .min(60usize.max(limit))
        .min(result_count)
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
