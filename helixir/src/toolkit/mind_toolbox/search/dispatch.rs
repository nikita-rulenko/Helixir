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

impl SearchEngine {
    pub async fn search(
        &self,
        query: &str,
        query_embedding: &[f32],
        user_id: &str,
        limit: usize,
        mode: &str,
        temporal_days: Option<f64>,
        graph_depth: Option<u32>,
        scope: &str,
    ) -> Result<Vec<UnifiedSearchResult>, SearchError> {
        self.search_windowed(
            query,
            query_embedding,
            user_id,
            limit,
            mode,
            temporal_days,
            graph_depth,
            scope,
            TimeWindow::default(),
        )
        .await
    }

    /// #87: search with an explicit two-sided EVENT-time window. When the
    /// window is inactive, `temporal_days` (or the mode default) becomes a
    /// one-sided window — the legacy cutoff. An active explicit window wins
    /// over `temporal_days`.
    #[allow(clippy::too_many_arguments)]
    pub async fn search_windowed(
        &self,
        query: &str,
        query_embedding: &[f32],
        user_id: &str,
        limit: usize,
        mode: &str,
        temporal_days: Option<f64>,
        graph_depth: Option<u32>,
        scope: &str,
        window: TimeWindow,
    ) -> Result<Vec<UnifiedSearchResult>, SearchError> {
        let query_preview: String = query.chars().take(30).collect();

        let search_mode = SearchMode::parse_mode(mode);
        let mode_defaults = self.config.retrieval.search_modes.for_mode(search_mode);
        let effective_temporal_days = temporal_days.or(mode_defaults.temporal_days);

        let window = if window.is_active() {
            window
        } else {
            match effective_temporal_days {
                Some(days) => TimeWindow::last_days(days, chrono::Utc::now()),
                None => TimeWindow::default(),
            }
        };
        let flashback_max = if window.is_active() {
            self.config.retrieval.flashback_max
        } else {
            0
        };

        let effective_user_id: Option<&str> = match scope {
            "collective" | "all" => None,
            _ => Some(user_id),
        };

        info!(
            "SearchEngine.search: query='{}...', user={}, mode={}, limit={}, scope={}, window={:?}..{:?}",
            query_preview, user_id, mode, limit, scope, window.from, window.to
        );

        // The cross-user cache is keyed by embedding only — a windowed
        // result set must not be served to (or poison) unwindowed callers.
        let cross_user_cacheable = effective_user_id.is_none() && !window.is_active();
        if cross_user_cacheable {
            let cache_key = embedding_cache_key(query_embedding);
            if let Some(cached) = self.cross_user_cache.get(&cache_key).await {
                info!("Cross-user cache hit for scope={}", scope);
                return Ok(cached);
            }
        }

        let results = match mode.to_lowercase().as_str() {
            "recent" | "contextual" => {
                if let Some(ref traversal) = self.smart_traversal {
                    debug!(
                        "Using SmartTraversalV2 for mode={}, window={:?}..{:?}, scope={}",
                        mode, window.from, window.to, scope
                    );
                    let config = self.make_search_config(
                        limit,
                        // #8: explicit graph_depth overrides the mode default
                        // (capped at 4 — the full-mode maximum).
                        graph_depth
                            .map(|d| d.clamp(1, 4))
                            .unwrap_or(if mode == "recent" { 1 } else { 2 }),
                        mode_defaults.min_vector_score,
                        mode_defaults.min_combined_score,
                        mode_defaults.temporal_weight,
                    );
                    let traversal_results = traversal
                        .search(query, query_embedding, effective_user_id, config, window)
                        .await
                        .unwrap_or_default();

                    // #81/#36: honest limit — graph expansion inflates the
                    // seed set (depth 2 turned 8 seeds into 114 rows for a
                    // think_recall) and, unlike the deep branch, nothing
                    // clamped here. Dedup by memory_id first (the same memory
                    // arrives as a seed AND as an expansion child, and dups
                    // would eat slots of the clamped window); results are
                    // sorted by combined score, so the first occurrence wins.
                    // #87: flashbacks live in their own small allowance.
                    let mapped: Vec<UnifiedSearchResult> = traversal_results
                        .into_iter()
                        .map(|r| UnifiedSearchResult {
                            memory_id: r.memory_id,
                            content: r.content,
                            score: r.combined_score as f32,
                            method: format!("smart_v2_{}", mode),
                            metadata: r.metadata.unwrap_or_default(),
                            created_at: r.created_at.unwrap_or_default(),
                            user_count: None,
                            controversy: None,
                        })
                        .collect();
                    clamp_with_flashbacks(mapped, limit, flashback_max)
                } else {
                    self.vector_search_unified(query, effective_user_id, limit)
                        .await?
                }
            }
            "deep" => {
                if let Some(ref traversal) = self.smart_traversal {
                    debug!(
                        "Using SmartTraversalV2 for deep search, window={:?}..{:?}, scope={}",
                        window.from, window.to, scope
                    );
                    let config = self.make_search_config(
                        limit * 2,
                        graph_depth.map(|d| d.clamp(1, 4)).unwrap_or(3),
                        self.config.search_thresholds.min_vector_score,
                        mode_defaults.min_combined_score,
                        mode_defaults.temporal_weight,
                    );
                    let traversal_results = traversal
                        .search(query, query_embedding, effective_user_id, config, window)
                        .await
                        .unwrap_or_default();

                    // Same dedup-before-clamp as the recent/contextual branch:
                    // duplicate rows (seed + expansion) must not eat slots.
                    let mapped: Vec<UnifiedSearchResult> = traversal_results
                        .into_iter()
                        .map(|r| UnifiedSearchResult {
                            memory_id: r.memory_id,
                            content: r.content,
                            score: r.combined_score as f32,
                            method: "smart_v2_deep".to_string(),
                            metadata: r.metadata.unwrap_or_default(),
                            created_at: r.created_at.unwrap_or_default(),
                            user_count: None,
                            controversy: None,
                        })
                        .collect();
                    clamp_with_flashbacks(mapped, limit, flashback_max)
                } else {
                    self.vector_search_unified(query, effective_user_id, limit)
                        .await?
                }
            }
            "full" => {
                if let Some(ref traversal) = self.smart_traversal {
                    // #31: full mode has no IMPLICIT window (presets are None
                    // everywhere now), but an EXPLICIT temporal_days or
                    // time window is the caller asking for a hard filter —
                    // honor it here too.
                    debug!(
                        "Using SmartTraversalV2 for full mode, window={:?}..{:?}, scope={}",
                        window.from, window.to, scope
                    );
                    let config = self.make_search_config(
                        limit * 2,
                        graph_depth.map(|d| d.clamp(1, 4)).unwrap_or(4),
                        self.config.search_thresholds.min_vector_score,
                        self.config.search_thresholds.min_combined_score,
                        mode_defaults.temporal_weight,
                    );
                    let traversal_results = traversal
                        .search(query, query_embedding, effective_user_id, config, window)
                        .await
                        .unwrap_or_default();

                    // Same dedup-before-clamp as the other traversal branches:
                    // duplicate rows (seed + expansion) must not eat slots of
                    // the clamped window.
                    let mapped: Vec<UnifiedSearchResult> = traversal_results
                        .into_iter()
                        .map(|r| UnifiedSearchResult {
                            memory_id: r.memory_id,
                            content: r.content,
                            score: r.combined_score as f32,
                            method: "smart_v2_full".to_string(),
                            metadata: r.metadata.unwrap_or_default(),
                            created_at: r.created_at.unwrap_or_default(),
                            user_count: None,
                            controversy: None,
                        })
                        .collect();
                    clamp_with_flashbacks(mapped, limit, flashback_max)
                } else {
                    debug!("SmartTraversal not available, returning empty for full mode");
                    Vec::new()
                }
            }
            _ => {
                debug!("Unknown mode '{}', falling back to vector search", mode);
                self.vector_search_unified(query, effective_user_id, limit)
                    .await?
            }
        };

        let mut final_results = results;

        if (scope == "collective" || scope == "all") && !final_results.is_empty() {
            let enrichment_futures: Vec<_> = final_results
                .iter()
                .map(|r| {
                    let mem_id = r.memory_id.clone();
                    let uid = user_id.to_string();
                    let client = Arc::clone(&self.client);
                    async move {
                        let user_count = Self::fetch_memory_user_count_static(&client, &mem_id)
                            .await
                            .ok();
                        let controversy = Self::fetch_controversy_static(&client, &mem_id, &uid)
                            .await
                            .ok()
                            .flatten();
                        // Cognitive layer (#33): who relates to this fact and how.
                        let stances = Self::fetch_memory_stances_static(&client, &mem_id)
                            .await
                            .ok()
                            .filter(|d| !d.is_empty());
                        (mem_id, user_count, controversy, stances)
                    }
                })
                .collect();

            let enrichments = futures::future::join_all(enrichment_futures).await;
            for (mem_id, user_count, controversy, stances) in enrichments {
                if let Some(r) = final_results.iter_mut().find(|r| r.memory_id == mem_id) {
                    r.user_count = user_count;
                    r.controversy = controversy;
                    if let Some(distribution) = stances {
                        if let Ok(value) = serde_json::to_value(&distribution) {
                            r.metadata.insert("stances".to_string(), value);
                        }
                    }
                }
            }
        }

        if cross_user_cacheable {
            let cache_key = embedding_cache_key(query_embedding);
            self.cross_user_cache
                .insert(cache_key, final_results.clone())
                .await;
        }

        info!(
            "SearchEngine.search complete: {} results (scope={})",
            final_results.len(),
            scope
        );
        Ok(final_results)
    }

    /// #82: collapse raw+atom families inside one result window. For every
    /// `raw_*` row present, fetch its incoming PART_OF edges (the atom→raw
    /// family link written by the add pipeline) and, when family members
    /// share the window, keep only the best-ranked one — annotated with the
    /// folded ids under `metadata.collapsed`. Zero cost when no raw row is
    /// in the window (the overwhelmingly common case).
    /// NOTE: deliberately NOT called inside [`SearchEngine::search`] — the
    /// write path's dedup recall (Phase A) needs the RAW candidates visible,
    /// or the duplicate gate loses the very atom it must compare against.
    /// The presentation layer (ToolingManager::search) calls this instead.
    pub async fn collapse_raw_families(&self, results: &mut Vec<UnifiedSearchResult>) {
        let raw_ids: Vec<String> = results
            .iter()
            .filter(|r| r.memory_id.starts_with("raw_"))
            .map(|r| r.memory_id.clone())
            .collect();
        if raw_ids.is_empty() {
            return;
        }

        let mut drop_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut annotations: Vec<(String, Vec<String>)> = Vec::new();

        for raw_id in raw_ids {
            // Two lookups joined on the internal node id: the EDGE projection
            // carries relation_type but only internal from_node UUIDs, while
            // the NODE projection carries memory_id + internal id. Both are
            // existing queries — no schema change.
            let edges: serde_json::Value = match self
                .client
                .execute_query("getMemoryIncomingRelations", &json!({"memory_id": &raw_id}))
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    debug!("family edge lookup failed for {}: {}", raw_id, e);
                    continue;
                }
            };
            let part_of_nodes: std::collections::HashSet<String> = edges["relations_in"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter(|e| e["relation_type"].as_str() == Some("PART_OF"))
                        .filter_map(|e| e["from_node"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            if part_of_nodes.is_empty() {
                continue;
            }
            let nodes: serde_json::Value = match self
                .client
                .execute_query(
                    "getMemoryLogicalConnections",
                    &json!({"memory_id": &raw_id}),
                )
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    debug!("family node lookup failed for {}: {}", raw_id, e);
                    continue;
                }
            };
            let family: std::collections::HashSet<String> = nodes["relation_in"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter(|n| {
                            n["id"]
                                .as_str()
                                .is_some_and(|id| part_of_nodes.contains(id))
                        })
                        .filter_map(|n| n["memory_id"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            if family.is_empty() {
                continue;
            }

            // Members of this family present in the window, best score first
            // (results are already rank-ordered).
            let present: Vec<String> = results
                .iter()
                .filter(|r| r.memory_id == raw_id || family.contains(&r.memory_id))
                .map(|r| r.memory_id.clone())
                .collect();
            if present.len() < 2 {
                continue;
            }
            // Content-lossless folding only. Sibling ATOMS are distinct
            // facts and must never fold into each other; the raw↔atom pair
            // is the only true redundancy (atom content is contained in the
            // raw). So: best member is an atom → fold ONLY the raw; best
            // member is the raw → fold the present atoms (their content is
            // inside the kept raw).
            let keeper = present[0].clone();
            let folded: Vec<String> = if keeper == raw_id {
                present.into_iter().skip(1).collect()
            } else {
                vec![raw_id.clone()]
            };
            drop_ids.extend(folded.iter().cloned());
            annotations.push((keeper, folded));
        }

        if drop_ids.is_empty() {
            return;
        }
        results.retain(|r| !drop_ids.contains(&r.memory_id));
        for (keeper, folded) in annotations {
            if let Some(row) = results.iter_mut().find(|r| r.memory_id == keeper) {
                row.metadata.insert("collapsed".to_string(), json!(folded));
            }
        }
    }

    pub async fn search_for_dedup(
        &self,
        query: &str,
        query_embedding: &[f32],
        user_id: &str,
        limit: usize,
    ) -> Result<Vec<UnifiedSearchResult>, SearchError> {
        let query_preview: String = query.chars().take(30).collect();
        info!(
            "SearchEngine.search_for_dedup: query='{}...', user={}, limit={}",
            query_preview, user_id, limit
        );

        if let Some(ref traversal) = self.smart_traversal {
            let config = self.make_search_config(
                limit,
                2,
                self.config.search_thresholds.min_vector_score,
                self.config.search_thresholds.min_combined_score,
                self.config.search_thresholds.temporal_weight,
            );
            let results = traversal
                .search(query, query_embedding, None, config, TimeWindow::default())
                .await
                .unwrap_or_default();

            Ok(results
                .into_iter()
                .take(limit)
                .map(|r| UnifiedSearchResult {
                    memory_id: r.memory_id,
                    content: r.content,
                    score: r.combined_score as f32,
                    method: "dedup_collective".to_string(),
                    metadata: r.metadata.unwrap_or_default(),
                    created_at: r.created_at.unwrap_or_default(),
                    user_count: None,
                    controversy: None,
                })
                .collect())
        } else {
            self.vector_search_unified(query, None, limit).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, score: f32, flashback: bool) -> UnifiedSearchResult {
        let mut metadata = std::collections::HashMap::new();
        if flashback {
            metadata.insert("flashback".to_string(), serde_json::Value::Bool(true));
        }
        UnifiedSearchResult {
            memory_id: id.to_string(),
            content: format!("content {id}"),
            score,
            method: "test".to_string(),
            metadata,
            created_at: String::new(),
            user_count: None,
            controversy: None,
        }
    }

    #[test]
    fn flashbacks_never_crowd_in_window_rows() {
        // 4 in-window + 5 flashbacks, limit 4, allowance 2: all 4 in-window
        // rows survive; only 2 best flashbacks append after them.
        let mut input: Vec<UnifiedSearchResult> =
            (0..4).map(|i| row(&format!("in{i}"), 0.9, false)).collect();
        input.extend((0..5).map(|i| row(&format!("fb{i}"), 0.95, true)));
        let out = clamp_with_flashbacks(input, 4, 2);
        assert_eq!(out.len(), 6);
        assert!(out[..4].iter().all(|r| r.memory_id.starts_with("in")));
        assert!(out[4..].iter().all(|r| r.memory_id.starts_with("fb")));
    }

    #[test]
    fn zero_allowance_drops_all_flashbacks() {
        let input = vec![row("a", 0.9, false), row("f", 0.8, true)];
        let out = clamp_with_flashbacks(input, 5, 0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].memory_id, "a");
    }

    #[test]
    fn dedup_still_runs_before_the_clamp() {
        // The same memory as seed AND expansion child must not eat a slot.
        let input = vec![
            row("dup", 0.9, false),
            row("dup", 0.7, false),
            row("b", 0.6, false),
        ];
        let out = clamp_with_flashbacks(input, 2, 0);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].memory_id, "dup");
        assert_eq!(out[1].memory_id, "b");
    }
}
