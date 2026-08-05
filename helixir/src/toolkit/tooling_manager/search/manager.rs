//! Tooling-manager search operations.

use super::*;

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
                    internal_id: r.internal_id,
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
                internal_id: None,
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
                            internal_id: candidate.internal_id.clone(),
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
                                internal_id: None,
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
