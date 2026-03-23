use std::collections::HashMap;

use serde::{Deserialize, Deserializer};
use tracing::{info, debug};

use crate::utils::nullable_string;
use super::helpers::safe_truncate;
use super::types::{SearchMemoryResult, ToolingError};
use super::ToolingManager;

impl ToolingManager {
    pub async fn search_memory(
        &self,
        query: &str,
        user_id: &str,
        limit: Option<usize>,
        mode: &str,
        temporal_days: Option<f64>,
        _graph_depth: Option<usize>,
        scope: &str,
    ) -> Result<Vec<SearchMemoryResult>, ToolingError> {
        info!(
            "Searching: '{}...' [mode={}, limit={:?}, temporal_days={:?}, scope={}]",
            safe_truncate(query, 50), mode, limit, temporal_days, scope
        );

        let query_embedding = self
            .embedder
            .generate(query, true)
            .await
            .map_err(|e| ToolingError::Embedding(e.to_string()))?;

        let effective_limit = limit.unwrap_or(10);

        let results = match scope {
            "collective" | "all" => {
                self.search_engine
                    .search(query, &query_embedding, user_id, effective_limit, mode, temporal_days, scope)
                    .await?
            }
            _ => {
                self.search_engine
                    .search(query, &query_embedding, user_id, effective_limit, mode, temporal_days, "personal")
                    .await?
            }
        };

        self.emit_search_executed(user_id, mode, results.len()).await;

        info!("Found {} memories via SearchEngine [method={}, scope={}]",
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
            search_results.sort_by(|a, b| {
                let a_uc = a.metadata.get("user_count")
                    .and_then(|v| v.as_u64()).unwrap_or(1);
                let b_uc = b.metadata.get("user_count")
                    .and_then(|v| v.as_u64()).unwrap_or(1);
                let a_combined = a.score * (1.0 + (a_uc as f64 - 1.0) * 0.1);
                let b_combined = b.score * (1.0 + (b_uc as f64 - 1.0) * 0.1);
                b_combined.partial_cmp(&a_combined).unwrap_or(std::cmp::Ordering::Equal)
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

        info!("Found {} memories with tag '{}'", result.memories.len(), tag);

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
        info!("Concept search: '{}...' type={:?} tags={:?}",
            safe_truncate(query, 30), concept_type, tags);

        let query_embedding = self
            .embedder
            .generate(query, true)
            .await
            .map_err(|e| ToolingError::Embedding(e.to_string()))?;

        let candidates = self
            .search_engine
            .search(query, &query_embedding, user_id, limit * 3, mode, None, "personal")
            .await?;

        let mut results = Vec::new();

        if !candidates.is_empty() {
            for candidate in &candidates {
                #[derive(serde::Deserialize)]
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

                if let Ok(concepts) = self.db
                    .execute_query::<ConceptsResult, _>(
                        "getMemoryConcepts",
                        &serde_json::json!({"memory_id": candidate.memory_id}),
                    )
                    .await
                {
                    let matches_type = match concept_type {
                        Some(ct) => {
                            let has_db_link = concepts.instance_of.iter().any(|c|
                                c.name.to_lowercase() == ct.to_lowercase() ||
                                c.concept_id.to_lowercase().contains(&ct.to_lowercase())
                            );

                            if has_db_link {
                                true
                            } else {
                                let memory_type = self.get_memory_type(&candidate.memory_id).await;
                                let type_matches = memory_type.as_ref()
                                    .map(|mt| mt.to_lowercase() == ct.to_lowercase())
                                    .unwrap_or(false);

                                if type_matches {
                                    true
                                } else {
                                    let ontology = self.ontology_manager.read();
                                    if ontology.is_loaded() {
                                        let mapped = ontology.map_memory_to_concepts(
                                            &candidate.content,
                                            memory_type.as_deref(),
                                        );
                                        mapped.iter().any(|m|
                                            m.concept.name.to_lowercase() == ct.to_lowercase() ||
                                            m.concept.id.to_lowercase() == ct.to_lowercase()
                                        )
                                    } else {
                                        false
                                    }
                                }
                            }
                        }
                        None => true,
                    };

                    let matches_tags = match tags {
                        Some(t) => {
                            let tag_list: Vec<&str> = t.split(',').map(|s| s.trim()).collect();
                            tag_list.iter().any(|tag|
                                candidate.content.to_lowercase().contains(&tag.to_lowercase())
                            )
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
            debug!("Vector search yielded no concept matches for type='{}', falling back to getUserMemories", ct);

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
            }

            let fetch_limit = (limit * 5).max(50) as i64;
            if let Ok(fallback) = self.db
                .execute_query::<FallbackMemoriesResult, _>(
                    "getUserMemories",
                    &serde_json::json!({"user_id": user_id, "limit": fetch_limit}),
                )
                .await
            {
                let ct_lower = ct.to_lowercase();
                for mem in fallback.memories {
                    if mem.memory_type.to_lowercase() == ct_lower {
                        let matches_tags = match tags {
                            Some(t) => {
                                let tag_list: Vec<&str> = t.split(',').map(|s| s.trim()).collect();
                                tag_list.iter().any(|tag|
                                    mem.content.to_lowercase().contains(&tag.to_lowercase())
                                )
                            }
                            None => true,
                        };

                        if matches_tags {
                            results.push(SearchMemoryResult {
                                memory_id: mem.memory_id,
                                content: mem.content,
                                score: 0.75,
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
                debug!("DB fallback found {} results for type='{}'", results.len(), ct);
            }
        }

        info!("Concept search found {} results", results.len());
        Ok(results)
    }
}
