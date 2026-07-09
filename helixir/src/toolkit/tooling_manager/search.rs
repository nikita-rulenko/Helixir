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
        let key = content_key(&r.content, mtype);
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
        if let Some(&c) = count.get(key) {
            if c > 1 {
                out[i]
                    .metadata
                    .insert("collapsed_holders".to_string(), serde_json::json!(c));
            }
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

impl ToolingManager {
    pub async fn search_memory(
        &self,
        query: &str,
        user_id: &str,
        opts: MemorySearchOptions,
    ) -> Result<Vec<SearchMemoryResult>, ToolingError> {
        let MemorySearchOptions {
            limit,
            mode,
            temporal_days,
            graph_depth,
            scope,
            window,
        } = opts;
        let (mode, scope) = (mode.as_str(), scope.as_str());
        info!(
            "Searching: '{}...' [mode={}, limit={:?}, temporal_days={:?}, window={:?}..{:?}, scope={}]",
            safe_truncate(query, 50),
            mode,
            limit,
            temporal_days,
            window.from,
            window.to,
            scope
        );

        let query_embedding = self
            .embedder
            .generate(query, true)
            .await
            .map_err(|e| ToolingError::Embedding(e.to_string()))?;

        let graph_depth = graph_depth.map(|d| d as u32);
        let effective_limit = limit.unwrap_or(self.config.default_search_limit);

        let effective_scope = match scope {
            "collective" | "all" => scope,
            _ => "personal",
        };
        let results = self
            .search_engine
            .search(
                query,
                &query_embedding,
                user_id,
                crate::toolkit::mind_toolbox::search::SearchOptions {
                    limit: effective_limit,
                    mode: mode.to_string(),
                    temporal_days,
                    graph_depth,
                    scope: effective_scope.to_string(),
                    window,
                },
            )
            .await?;

        // #82: presentation-layer family collapse — a raw source and its
        // extracted atoms in one window bill the same content twice. Done
        // HERE and not inside SearchEngine::search so internal consumers
        // (the write path's dedup recall) keep seeing raw candidates.
        let mut results = results;
        self.search_engine.collapse_raw_families(&mut results).await;

        self.emit_search_executed(user_id, mode, results.len())
            .await;

        info!(
            "Found {} memories via SearchEngine [method={}, scope={}]",
            results.len(),
            results.first().map(|r| r.method.as_str()).unwrap_or("none"),
            scope
        );

        let mut search_results: Vec<SearchMemoryResult> = results
            .into_iter()
            .map(|r| {
                let mut result = SearchMemoryResult {
                    memory_id: r.memory_id,
                    content: r.content,
                    score: r.score as f64,
                    method: r.method,
                    metadata: r.metadata,
                    created_at: r.created_at,
                };
                if let Some(uc) = r.user_count {
                    result.metadata.insert(
                        "user_count".to_string(),
                        serde_json::Value::Number(serde_json::Number::from(uc)),
                    );
                }
                if let Some(ref controversy) = r.controversy {
                    result.metadata.insert(
                        "controversy".to_string(),
                        serde_json::to_value(controversy).unwrap_or_default(),
                    );
                }
                result
            })
            .collect();

        if scope == "collective" || scope == "all" {
            // #3a: fold same-fact-across-users into one row BEFORE ranking, so
            // the boost-sort operates on distinct knowledge, not duplicates.
            let before = search_results.len();
            search_results = collapse_collective_duplicates(search_results);
            if search_results.len() < before {
                debug!(
                    "collective dedup: collapsed {} rows -> {} distinct facts",
                    before,
                    search_results.len()
                );
            }
            let boost = self.config.retrieval.collective_user_count_boost;
            search_results.sort_by(|a, b| {
                let a_uc = a
                    .metadata
                    .get("user_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1);
                let b_uc = b
                    .metadata
                    .get("user_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1);
                let a_combined = a.score * (1.0 + (a_uc as f64 - 1.0) * boost);
                let b_combined = b.score * (1.0 + (b_uc as f64 - 1.0) * boost);
                b_combined
                    .partial_cmp(&a_combined)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        Ok(search_results)
    }

    pub async fn search_by_tag(
        &self,
        tag: &str,
        limit: usize,
    ) -> Result<Vec<SearchMemoryResult>, ToolingError> {
        info!("Searching by tag: {} [limit={}]", tag, limit);

        #[derive(serde::Deserialize)]
        #[allow(dead_code)] // `context_tags` reflected from HelixDB; surfaced through diagnostics.
        struct TaggedMemory {
            #[serde(default, deserialize_with = "nullable_string")]
            memory_id: String,
            #[serde(default, deserialize_with = "nullable_string")]
            content: String,
            #[serde(default, deserialize_with = "nullable_string")]
            context_tags: String,
            #[serde(default, deserialize_with = "nullable_string")]
            created_at: String,
        }

        #[derive(serde::Deserialize)]
        struct QueryResult {
            memories: Vec<TaggedMemory>,
        }

        let result: QueryResult = self
            .db
            .execute_query(
                "searchByContextTag",
                &serde_json::json!({
                    "tag": tag,
                    "limit": limit as i64
                }),
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        info!(
            "Found {} memories with tag '{}'",
            result.memories.len(),
            tag
        );

        Ok(result
            .memories
            .into_iter()
            .map(|m| SearchMemoryResult {
                memory_id: m.memory_id,
                content: m.content,
                score: 1.0,
                method: "tag_search".to_string(),
                metadata: HashMap::new(),
                created_at: m.created_at,
            })
            .collect())
    }

    pub async fn search_by_concept(
        &self,
        query: &str,
        user_id: &str,
        concept_type: Option<&str>,
        tags: Option<&str>,
        mode: &str,
        limit: usize,
    ) -> Result<Vec<SearchMemoryResult>, ToolingError> {
        info!(
            "Concept search: '{}...' type={:?} tags={:?}",
            safe_truncate(query, 30),
            concept_type,
            tags
        );

        let query_embedding = self
            .embedder
            .generate(query, true)
            .await
            .map_err(|e| ToolingError::Embedding(e.to_string()))?;

        let candidates = self
            .search_engine
            .search(
                query,
                &query_embedding,
                user_id,
                crate::toolkit::mind_toolbox::search::SearchOptions::new(limit * 3, mode),
            )
            .await?;

        let mut results = Vec::new();

        if !candidates.is_empty() {
            for candidate in &candidates {
                #[derive(serde::Deserialize)]
                #[allow(dead_code)] // `belongs_to` paired with `instance_of`; the latter is iterated below.
                struct ConceptsResult {
                    #[serde(default)]
                    instance_of: Vec<ConceptNode>,
                    #[serde(default)]
                    belongs_to: Vec<ConceptNode>,
                }

                #[derive(serde::Deserialize)]
                struct ConceptNode {
                    #[serde(default, deserialize_with = "nullable_string")]
                    concept_id: String,
                    #[serde(default, deserialize_with = "nullable_string")]
                    name: String,
                }

                if let Ok(concepts) = self
                    .db
                    .execute_query::<ConceptsResult, _>(
                        "getMemoryConcepts",
                        &serde_json::json!({"memory_id": candidate.memory_id}),
                    )
                    .await
                {
                    let matches_type = match concept_type {
                        Some(ct) => {
                            // Exact match only — `contains` used to leak (ct "fact"
                            // matched concept_id "artifact"), #62.
                            let has_db_link = concepts.instance_of.iter().any(|c| {
                                c.name.eq_ignore_ascii_case(ct)
                                    || c.concept_id.eq_ignore_ascii_case(ct)
                            });

                            if has_db_link {
                                true
                            } else {
                                match self.get_memory_type(&candidate.memory_id).await {
                                    // A memory with a KNOWN type matches ONLY if it
                                    // IS that type — never fall through to the fuzzy
                                    // ontology mapping, which pulled in adjacent
                                    // ontology types via graph expansion (#62).
                                    Some(mt) => mt.eq_ignore_ascii_case(ct),
                                    // Unknown type: last-resort ontology mapping.
                                    None => {
                                        let ontology = self.ontology_manager.read();
                                        ontology.is_loaded()
                                            && ontology
                                                .map_memory_to_concepts(&candidate.content, None)
                                                .iter()
                                                .any(|m| {
                                                    m.concept.name.eq_ignore_ascii_case(ct)
                                                        || m.concept.id.eq_ignore_ascii_case(ct)
                                                })
                                    }
                                }
                            }
                        }
                        None => true,
                    };

                    let matches_tags = match tags {
                        Some(t) => {
                            let tag_list: Vec<&str> = t.split(',').map(|s| s.trim()).collect();
                            tag_list.iter().any(|tag| {
                                candidate
                                    .content
                                    .to_lowercase()
                                    .contains(&tag.to_lowercase())
                            })
                        }
                        None => true,
                    };

                    if matches_type && matches_tags {
                        results.push(SearchMemoryResult {
                            memory_id: candidate.memory_id.clone(),
                            content: candidate.content.clone(),
                            score: candidate.score as f64,
                            method: format!("concept_search_{}", mode),
                            metadata: candidate.metadata.clone(),
                            created_at: candidate.created_at.clone(),
                        });

                        if results.len() >= limit {
                            break;
                        }
                    }
                }
            }
        }

        if let Some(ct) = concept_type.filter(|_| results.is_empty()) {
            debug!(
                "Vector search yielded no concept matches for type='{}', falling back to getUserMemories",
                ct
            );

            #[derive(serde::Deserialize)]
            struct FallbackMemoriesResult {
                #[serde(default)]
                memories: Vec<FallbackMemory>,
            }
            #[derive(serde::Deserialize)]
            struct FallbackMemory {
                #[serde(default, deserialize_with = "nullable_string")]
                memory_id: String,
                #[serde(default, deserialize_with = "nullable_string")]
                content: String,
                #[serde(default, deserialize_with = "nullable_string")]
                memory_type: String,
                #[serde(default, deserialize_with = "nullable_string")]
                created_at: String,
                #[serde(default)]
                certainty: i64,
                #[serde(default)]
                importance: i64,
            }

            let fetch_limit = (limit * 5).max(50) as i64;
            if let Ok(fallback) = self
                .db
                .execute_query::<FallbackMemoriesResult, _>(
                    "getUserMemories",
                    &serde_json::json!({"user_id": user_id, "limit": fetch_limit}),
                )
                .await
            {
                let ct_lower = ct.to_lowercase();
                let query_lower = query.to_lowercase();
                for mem in fallback.memories {
                    if mem.memory_type.to_lowercase() == ct_lower {
                        let matches_tags = match tags {
                            Some(t) => {
                                let tag_list: Vec<&str> = t.split(',').map(|s| s.trim()).collect();
                                tag_list.iter().any(|tag| {
                                    mem.content.to_lowercase().contains(&tag.to_lowercase())
                                })
                            }
                            None => true,
                        };

                        if matches_tags {
                            // Real score: combine token overlap with the
                            // memory's own importance/certainty. Replaces the
                            // hard-coded 0.75 constant that made the field
                            // useless for ranking. See issue #22.
                            let score = concept_fallback_score(
                                &query_lower,
                                &mem.content,
                                mem.importance,
                                mem.certainty,
                            );

                            results.push(SearchMemoryResult {
                                memory_id: mem.memory_id,
                                content: mem.content,
                                score,
                                method: "concept_search_db_fallback".to_string(),
                                metadata: HashMap::new(),
                                created_at: mem.created_at,
                            });

                            if results.len() >= limit {
                                break;
                            }
                        }
                    }
                }
                // Sort DB-fallback results by descending score so the response
                // is monotone-relevant — without this the ordering reflects
                // HelixDB insertion order, which is meaningless to callers.
                results.sort_by(|a, b| {
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                debug!(
                    "DB fallback found {} results for type='{}'",
                    results.len(),
                    ct
                );
            }
        }

        info!("Concept search found {} results", results.len());
        Ok(results)
    }
}

/// Score function used in the `search_by_concept` DB fallback path.
///
/// We don't have a real vector similarity at this point (we already fell
/// back to `getUserMemories` precisely because vector search returned
/// nothing), so the score is a deterministic mix of two cheap signals:
///
/// * **Token-overlap** between the query and the memory content
///   (`|q ∩ c| / |q|`). Cheap, language-agnostic, and good enough to
///   discriminate "this memory is on-topic" from "this memory happens to
///   share a `memory_type`".
/// * **Author's own importance + certainty** averaged into a [0, 1]
///   confidence proxy. Stops near-zero-overlap matches from being ranked
///   above well-attested but slightly off-topic ones.
///
/// The final score is `0.7 * overlap + 0.3 * confidence`, clamped to
/// `[0, 1]`. Replaces the constant `0.75` from issue #22.
fn concept_fallback_score(
    query_lower: &str,
    memory_content: &str,
    importance: i64,
    certainty: i64,
) -> f64 {
    let query_tokens: std::collections::HashSet<&str> = query_lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 2)
        .collect();

    let overlap = if query_tokens.is_empty() {
        0.0
    } else {
        let content_lower = memory_content.to_lowercase();
        let hit = query_tokens
            .iter()
            .filter(|t| content_lower.contains(*t))
            .count();
        hit as f64 / query_tokens.len() as f64
    };

    let importance = importance.clamp(0, 100) as f64 / 100.0;
    let certainty = certainty.clamp(0, 100) as f64 / 100.0;
    let confidence = (importance + certainty) / 2.0;

    (0.7 * overlap + 0.3 * confidence).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::SearchMemoryResult;
    use super::{collapse_collective_duplicates, concept_fallback_score};
    use std::collections::HashMap;

    fn res(memory_id: &str, content: &str, mtype: &str, score: f64) -> SearchMemoryResult {
        let mut metadata = HashMap::new();
        if !mtype.is_empty() {
            metadata.insert("memory_type".to_string(), serde_json::json!(mtype));
        }
        SearchMemoryResult {
            memory_id: memory_id.to_string(),
            content: content.to_string(),
            score,
            method: "test".to_string(),
            metadata,
            created_at: String::new(),
        }
    }

    #[test]
    fn collapse_folds_same_fact_across_users_keeping_best() {
        // Two users hold the SAME fact (same content+type -> same content_key),
        // plus one distinct fact. Whitespace/case differences must still fold.
        let input = vec![
            res("mem_a", "Rust is a systems language", "fact", 0.80), // user A
            res("mem_b", "rust  is a   systems language", "fact", 0.91), // user B (higher)
            res("mem_c", "Postgres is a database", "fact", 0.70),     // distinct
        ];
        let out = collapse_collective_duplicates(input);
        assert_eq!(out.len(), 2, "the duplicated fact must collapse to one row");
        // The surviving representative is the higher-scored copy.
        let folded = out
            .iter()
            .find(|r| r.content.contains("systems language"))
            .expect("folded fact present");
        assert_eq!(folded.memory_id, "mem_b", "keep the highest-scored holder");
        assert_eq!(
            folded
                .metadata
                .get("collapsed_holders")
                .and_then(|v| v.as_u64()),
            Some(2),
            "collapsed_holders must reflect both holders"
        );
        // The distinct fact is untouched and not annotated.
        let distinct = out
            .iter()
            .find(|r| r.content.contains("Postgres"))
            .expect("distinct fact present");
        assert!(distinct.metadata.get("collapsed_holders").is_none());
    }

    #[test]
    fn collapse_distinguishes_by_memory_type() {
        // Same text but different ontology type -> different content_key -> NOT folded.
        let input = vec![
            res("mem_a", "I prefer dark mode", "preference", 0.9),
            res("mem_b", "I prefer dark mode", "fact", 0.8),
        ];
        let out = collapse_collective_duplicates(input);
        assert_eq!(out.len(), 2, "different memory_type must not collapse");
    }

    #[test]
    fn concept_fallback_score_rewards_token_overlap() {
        let high = concept_fallback_score("rust gen keyword", "Rust 2024 reserves gen.", 80, 90);
        let low =
            concept_fallback_score("rust gen keyword", "I like coffee in the morning.", 80, 90);
        assert!(
            high > low,
            "overlap-heavy match must score above unrelated content: {high} <= {low}"
        );
    }

    #[test]
    fn concept_fallback_score_uses_importance_when_overlap_is_zero() {
        let strong =
            concept_fallback_score("alpha beta", "Completely unrelated content.", 100, 100);
        let weak = concept_fallback_score("alpha beta", "Completely unrelated content.", 0, 0);
        assert!(strong > weak);
        assert!(strong <= 1.0);
        assert!(weak >= 0.0);
    }

    #[test]
    fn concept_fallback_score_is_bounded() {
        // Saturating inputs must not let the score escape [0, 1].
        let high = concept_fallback_score("zzz", "zzz", 200, 200);
        assert!((0.0..=1.0).contains(&high));
    }

    #[test]
    fn concept_fallback_score_handles_empty_query() {
        // An empty query should produce a non-NaN, bounded fallback driven
        // entirely by importance/certainty.
        let s = concept_fallback_score("", "anything", 50, 50);
        assert!(s.is_finite());
        assert!((0.0..=1.0).contains(&s));
    }
}
