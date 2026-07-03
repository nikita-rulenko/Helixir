//! Top-level dispatch for [`super::SearchEngine`]:
//! [`SearchEngine::search`] (mode-driven user query) and
//! [`SearchEngine::search_for_dedup`] (lightweight cross-user dedup probe).

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use tracing::{debug, info};

use crate::core::search_modes::SearchMode;

use super::engine::{SearchEngine, embedding_cache_key};
use super::types::{SearchError, UnifiedSearchResult};

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
        let query_preview: String = query.chars().take(30).collect();

        let search_mode = SearchMode::parse_mode(mode);
        let mode_defaults = self.config.retrieval.search_modes.for_mode(search_mode);
        let effective_temporal_days = temporal_days.or(mode_defaults.temporal_days);

        let temporal_cutoff: Option<DateTime<Utc>> = effective_temporal_days.map(|days| {
            let millis = (days * 24.0 * 60.0 * 60.0 * 1000.0) as i64;
            Utc::now() - Duration::milliseconds(millis)
        });

        let effective_user_id: Option<&str> = match scope {
            "collective" | "all" => None,
            _ => Some(user_id),
        };

        info!(
            "SearchEngine.search: query='{}...', user={}, mode={}, limit={}, scope={}, temporal_days={:?}",
            query_preview, user_id, mode, limit, scope, effective_temporal_days
        );

        if effective_user_id.is_none() {
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
                        "Using SmartTraversalV2 for mode={}, temporal_cutoff={:?}, scope={}",
                        mode, temporal_cutoff, scope
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
                        .search(
                            query,
                            query_embedding,
                            effective_user_id,
                            config,
                            temporal_cutoff,
                        )
                        .await
                        .unwrap_or_default();

                    // #81/#36: honest limit — graph expansion inflates the
                    // seed set (depth 2 turned 8 seeds into 114 rows for a
                    // think_recall) and, unlike the deep branch, nothing
                    // clamped here. Dedup by memory_id first (the same memory
                    // arrives as a seed AND as an expansion child, and dups
                    // would eat slots of the clamped window); results are
                    // sorted by combined score, so the first occurrence wins
                    // and `take(limit)` keeps the best of seeds + expansion.
                    let mut seen = std::collections::HashSet::new();
                    traversal_results
                        .into_iter()
                        .filter(|r| seen.insert(r.memory_id.clone()))
                        .take(limit)
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
                        .collect()
                } else {
                    self.vector_search_unified(query, effective_user_id, limit)
                        .await?
                }
            }
            "deep" => {
                if let Some(ref traversal) = self.smart_traversal {
                    debug!(
                        "Using SmartTraversalV2 for deep search, temporal_cutoff={:?}, scope={}",
                        temporal_cutoff, scope
                    );
                    let config = self.make_search_config(
                        limit * 2,
                        graph_depth.map(|d| d.clamp(1, 4)).unwrap_or(3),
                        self.config.search_thresholds.min_vector_score,
                        mode_defaults.min_combined_score,
                        mode_defaults.temporal_weight,
                    );
                    let traversal_results = traversal
                        .search(
                            query,
                            query_embedding,
                            effective_user_id,
                            config,
                            temporal_cutoff,
                        )
                        .await
                        .unwrap_or_default();

                    // Same dedup-before-clamp as the recent/contextual branch:
                    // duplicate rows (seed + expansion) must not eat slots.
                    let mut seen = std::collections::HashSet::new();
                    traversal_results
                        .into_iter()
                        .filter(|r| seen.insert(r.memory_id.clone()))
                        .take(limit)
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
                        .collect()
                } else {
                    self.vector_search_unified(query, effective_user_id, limit)
                        .await?
                }
            }
            "full" => {
                if let Some(ref traversal) = self.smart_traversal {
                    // #31: full mode has no IMPLICIT window (presets are None
                    // everywhere now), but an EXPLICIT temporal_days is the
                    // caller asking for a hard window — honor it here too
                    // (this branch used to hardcode None and silently drop it).
                    debug!(
                        "Using SmartTraversalV2 for full mode, temporal_cutoff={:?}, scope={}",
                        temporal_cutoff, scope
                    );
                    let config = self.make_search_config(
                        limit * 2,
                        graph_depth.map(|d| d.clamp(1, 4)).unwrap_or(4),
                        self.config.search_thresholds.min_vector_score,
                        self.config.search_thresholds.min_combined_score,
                        mode_defaults.temporal_weight,
                    );
                    let traversal_results = traversal
                        .search(
                            query,
                            query_embedding,
                            effective_user_id,
                            config,
                            temporal_cutoff,
                        )
                        .await
                        .unwrap_or_default();

                    // Same dedup-before-clamp as the other traversal branches:
                    // duplicate rows (seed + expansion) must not eat slots of
                    // the clamped window.
                    let mut seen = std::collections::HashSet::new();
                    traversal_results
                        .into_iter()
                        .filter(|r| seen.insert(r.memory_id.clone()))
                        .take(limit)
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
                        .collect()
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

        if effective_user_id.is_none() {
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
                .search(query, query_embedding, None, config, None)
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
