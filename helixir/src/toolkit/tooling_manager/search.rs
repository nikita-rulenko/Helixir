use std::collections::HashMap;

use tracing::{debug, info};

use super::ToolingManager;
use super::add_pipeline::store::content_key;
use super::types::{SearchMemoryResult, ToolingError};
use crate::safe_truncate;
use crate::utils::nullable_string;

/// #3a: collapse same-`content_key` duplicates in a collective result set. Two
/// users holding the SAME fact are ONE piece of knowledge (consensus is per
/// content_key), so returning both is a fake duplicate. Keeps the highest-scored
/// representative per fingerprint group and records how many holders collapsed
/// into it via `collapsed_holders`. Pure (no I/O) so it is unit-tested directly.
fn collapse_collective_duplicates(results: Vec<SearchMemoryResult>) -> Vec<SearchMemoryResult> {
    let mut rep: HashMap<String, usize> = HashMap::new();
    let mut count: HashMap<String, u64> = HashMap::new();
    let mut out: Vec<SearchMemoryResult> = Vec::with_capacity(results.len());
    for r in results {
        let mtype = r
            .metadata
            .get("memory_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let key = r
            .metadata
            .get("content_key")
            .and_then(serde_json::Value::as_str)
            .filter(|key| !key.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| content_key(&r.content, mtype));
        *count.entry(key.clone()).or_insert(0) += 1;
        match rep.get(&key).copied() {
            // Already have a representative — keep whichever scored higher.
            Some(i) => {
                if r.score > out[i].score {
                    out[i] = r;
                }
            }
            None => {
                rep.insert(key.clone(), out.len());
                out.push(r);
            }
        }
    }
    // Surface the consensus: how many distinct holder-rows folded into each one.
    for (key, &i) in &rep {
        if let Some(&c) = count.get(key)
            && c > 1
        {
            out[i]
                .metadata
                .insert("collapsed_holders".to_string(), serde_json::json!(c));
        }
    }
    out
}

/// Tooling-level search request (#9): `mode` and `scope` arrive resolved by
/// the caller (the client layer); `limit` stays optional so the configured
/// default applies. #87: an active `window` hard-filters seeds by EVENT time;
/// graph expansion may pull out-of-window rows back in as flagged flashbacks
/// (`metadata.flashback` + `event_date`).
#[derive(Debug, Clone)]
pub struct MemorySearchOptions {
    pub limit: Option<usize>,
    pub mode: String,
    pub temporal_days: Option<f64>,
    pub graph_depth: Option<usize>,
    pub scope: String,
    pub window: crate::core::TimeWindow,
}

impl MemorySearchOptions {
    /// `mode` with the usual defaults: configured limit, personal scope,
    /// no temporal override, no window.
    pub fn new(mode: impl Into<String>) -> Self {
        Self {
            limit: None,
            mode: mode.into(),
            temporal_days: None,
            graph_depth: None,
            scope: "personal".to_string(),
            window: crate::core::TimeWindow::default(),
        }
    }
}

mod manager;
mod scoring;

use scoring::concept_fallback_score;

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
