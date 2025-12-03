

use std::num::NonZeroUsize;
use std::sync::Arc;

use lru::LruCache;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::db::HelixClient;
use crate::llm::providers::base::LlmProvider;


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReasoningType {
    
    Implies,
    
    Because,
    
    Contradicts,
    
    Supports,
}

impl ReasoningType {
    
    #[must_use]
    pub fn edge_name(&self) -> &'static str {
        match self {
            Self::Implies => "IMPLIES",
            Self::Because => "BECAUSE",
            Self::Contradicts => "CONTRADICTS",
            Self::Supports => "SUPPORTS",
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningRelation {
    
    pub relation_id: String,
    
    pub from_memory_id: String,
    
    pub to_memory_id: String,
    
    pub to_memory_content: String,
    
    pub relation_type: ReasoningType,
    
    pub strength: i32,
    
    pub reasoning_id: Option<String>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningChain {
    
    pub seed_memory_id: String,
    
    pub relations: Vec<ReasoningRelation>,
    
    pub chain_type: String,
    
    pub depth: usize,
    
    pub reasoning_trail: String,
}


pub struct ReasoningEngine {
    client: Arc<HelixClient>,
    llm_provider: Option<Arc<dyn LlmProvider>>,
    relation_cache: parking_lot::Mutex<LruCache<String, ReasoningRelation>>,
    cache_size: usize,
    is_warmed_up: std::sync::atomic::AtomicBool,
}

impl ReasoningEngine {
    
    #[must_use]
    pub fn new(
        client: Arc<HelixClient>,
        llm_provider: Option<Arc<dyn LlmProvider>>,
        cache_size: usize,
    ) -> Self {
        let cache = LruCache::new(
            NonZeroUsize::new(cache_size).unwrap_or(NonZeroUsize::new(1000).unwrap()),
        );

        info!(
            "ReasoningEngine initialized (cache_size={}, llm={})",
            cache_size,
            if llm_provider.is_some() {
                "enabled"
            } else {
                "disabled"
            }
        );

        Self {
            client,
            llm_provider,
            relation_cache: parking_lot::Mutex::new(cache),
            cache_size,
            is_warmed_up: std::sync::atomic::AtomicBool::new(false),
        }
    }

    
    pub async fn warm_up_cache(
        &self,
        memory_id: Option<&str>,
        limit: usize,
    ) -> Result<usize, ReasoningError> {
        use std::sync::atomic::Ordering;
        
        if self.is_warmed_up.load(Ordering::Relaxed) {
            info!("Reasoning cache already warmed up, skipping");
            return Ok(self.relation_cache.lock().len());
        }

        info!(
            "Warming up reasoning cache (memory={:?}, limit={})",
            memory_id, limit
        );

        #[derive(Deserialize)]
        struct QueryResult {
            relations: Option<Vec<serde_json::Value>>,
        }

        match self
            .client
            .execute_query::<QueryResult, _>(
                "getRecentRelations",
                &serde_json::json!({
                    "limit": limit,
                    "memory_id": memory_id,
                }),
            )
            .await
        {
            Ok(result) => {
                let relations = result.relations.map(|r| r.len()).unwrap_or(0);
                self.is_warmed_up.store(true, Ordering::Relaxed);
                info!("Cache warmup complete: {} relations loaded", relations);
                Ok(relations)
            }
            Err(e) => {
                debug!("Cache warmup skipped (query not available): {}", e);
                Ok(0)
            }
        }
    }

    
    pub async fn add_relation(
        &self,
        from_id: &str,
        to_id: &str,
        relation_type: ReasoningType,
        strength: i32,
        reasoning_id: Option<&str>,
    ) -> Result<ReasoningRelation, ReasoningError> {
        
        let strength = strength.clamp(0, 100);

        let relation = ReasoningRelation {
            relation_id: format!("rel_{}_{}", crate::safe_truncate(from_id, 8), crate::safe_truncate(to_id, 8)),
            from_memory_id: from_id.to_string(),
            to_memory_id: to_id.to_string(),
            to_memory_content: String::new(),
            relation_type,
            strength,
            reasoning_id: reasoning_id.map(String::from),
        };

        
        #[derive(Deserialize)]
        struct EdgeResponse {
            #[serde(default)]
            edge: serde_json::Value,
        }
        
        let persist_result = match relation_type {
            ReasoningType::Implies => {
                self.client
                    .execute_query::<EdgeResponse, _>(
                        "addMemoryImplication",
                        &serde_json::json!({
                            "from_id": from_id,
                            "to_id": to_id,
                            "probability": strength as i64,
                            "reasoning_id": reasoning_id.unwrap_or(""),
                        }),
                    )
                    .await
            }
            ReasoningType::Because => {
                self.client
                    .execute_query::<EdgeResponse, _>(
                        "addMemoryCausation",
                        &serde_json::json!({
                            "from_id": from_id,
                            "to_id": to_id,
                            "strength": strength as i64,
                            "reasoning_id": reasoning_id.unwrap_or(""),
                        }),
                    )
                    .await
            }
            ReasoningType::Contradicts => {
                self.client
                    .execute_query::<EdgeResponse, _>(
                        "addMemoryContradiction",
                        &serde_json::json!({
                            "from_id": from_id,
                            "to_id": to_id,
                            "resolution": "",
                            "resolved": 0i64,
                            "resolution_strategy": "pending",
                        }),
                    )
                    .await
            }
            ReasoningType::Supports => {
                
                let now = chrono::Utc::now().to_rfc3339();
                self.client
                    .execute_query::<EdgeResponse, _>(
                        "addReasoningRelation",
                        &serde_json::json!({
                            "relation_id": format!("rel_{}_{}", crate::safe_truncate(from_id, 8), crate::safe_truncate(to_id, 8)),
                            "from_memory_id": from_id,
                            "to_memory_id": to_id,
                            "relation_type": "SUPPORTS",
                            "strength": strength as i64,
                            "confidence": 80i64,
                            "explanation": "",
                            "created_by": "reasoning_engine",
                            "created_at": now,
                        }),
                    )
                    .await
            }
        };
        
        persist_result.map_err(|e| ReasoningError::Database(e.to_string()))?;

        
        self.relation_cache
            .lock()
            .put(relation.relation_id.clone(), relation.clone());

        debug!(
            "Added {} relation: {} -> {} (strength={})",
            relation_type.edge_name(),
            from_id,
            to_id,
            strength
        );

        Ok(relation)
    }

    
    pub async fn get_chain(
        &self,
        memory_id: &str,
        chain_type: &str,
        max_depth: usize,
    ) -> Result<ReasoningChain, ReasoningError> {
        #[derive(Deserialize)]
        struct ConnectionsResult {
            #[serde(default)]
            implies_out: Vec<MemoryNode>,
            #[serde(default)]
            implies_in: Vec<MemoryNode>,
            #[serde(default)]
            because_out: Vec<MemoryNode>,
            #[serde(default)]
            because_in: Vec<MemoryNode>,
            #[serde(default)]
            contradicts_out: Vec<MemoryNode>,
            #[serde(default)]
            contradicts_in: Vec<MemoryNode>,
            #[serde(default)]
            relation_out: Vec<MemoryNode>,
            #[serde(default)]
            relation_in: Vec<MemoryNode>,
        }
        
        #[derive(Deserialize, Clone)]
        struct MemoryNode {
            memory_id: String,
            #[serde(default)]
            content: String,
        }

        let mut relations = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut current_id = memory_id.to_string();
        let mut depth = 0;

        while depth < max_depth {
            if visited.contains(&current_id) {
                break;
            }
            visited.insert(current_id.clone());

            let result = match self
                .client
                .execute_query::<ConnectionsResult, _>(
                    "getMemoryLogicalConnections",
                    &serde_json::json!({"memory_id": &current_id}),
                )
                .await
            {
                Ok(r) => r,
                Err(_) => break,
            };

            let candidates: Vec<(MemoryNode, ReasoningType, bool)> = match chain_type {
                "causal" => {
                    result.because_in.iter()
                        .map(|n| (n.clone(), ReasoningType::Because, true))
                        .collect()
                }
                "forward" => {
                    result.implies_out.iter()
                        .map(|n| (n.clone(), ReasoningType::Implies, false))
                        .collect()
                }
                "both" | "deep" | _ => {
                    let mut all = Vec::new();
                    for n in &result.implies_out {
                        all.push((n.clone(), ReasoningType::Implies, false));
                    }
                    for n in &result.because_in {
                        all.push((n.clone(), ReasoningType::Because, true));
                    }
                    for n in &result.contradicts_out {
                        all.push((n.clone(), ReasoningType::Contradicts, false));
                    }
                    all
                }
            };

            let unvisited: Vec<_> = candidates
                .into_iter()
                .filter(|(n, _, _)| !visited.contains(&n.memory_id))
                .collect();

            if unvisited.is_empty() {
                break;
            }

            let best = if unvisited.len() == 1 {
                unvisited.into_iter().next()
            } else if let Some(llm) = &self.llm_provider {
                let prompt = format!(
                    "Given current memory and {} connected memories, which ONE is most logically relevant?\n\nCurrent: {}\n\nOptions:\n{}\n\nRespond with just the number (1-{}).",
                    unvisited.len(),
                    &current_id[..current_id.len().min(50)],
                    unvisited.iter().enumerate()
                        .map(|(i, (n, t, _))| format!("{}. [{}] {}", i + 1, t.edge_name(), n.content.chars().take(100).collect::<String>()))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    unvisited.len()
                );
                
                match llm.generate("You are a reasoning assistant. Pick the most relevant connection.", &prompt, None).await {
                    Ok((response, _)) => {
                        let choice: usize = response.trim().parse().unwrap_or(1);
                        unvisited.into_iter().nth(choice.saturating_sub(1))
                    }
                    Err(_) => unvisited.into_iter().next()
                }
            } else {
                unvisited.into_iter().next()
            };

            if let Some((node, relation_type, is_incoming)) = best {
                let (from_id, to_id) = if is_incoming {
                    (node.memory_id.clone(), current_id.clone())
                } else {
                    (current_id.clone(), node.memory_id.clone())
                };

                relations.push(ReasoningRelation {
                    relation_id: format!("rel_{}_{}", &from_id, &to_id),
                    from_memory_id: from_id,
                    to_memory_id: to_id,
                    to_memory_content: node.content.clone(),
                    relation_type,
                    strength: 80,
                    reasoning_id: None,
                });

                current_id = node.memory_id;
                depth += 1;
            } else {
                break;
            }
        }

        let reasoning_trail = self.build_reasoning_trail(&relations);

        Ok(ReasoningChain {
            seed_memory_id: memory_id.to_string(),
            relations,
            chain_type: chain_type.to_string(),
            depth,
            reasoning_trail,
        })
    }

    
    pub async fn infer_relations(
        &self,
        memory_id: &str,
        context_memories: &[String],
    ) -> Result<Vec<ReasoningRelation>, ReasoningError> {
        let Some(ref llm) = self.llm_provider else {
            return Ok(Vec::new());
        };

        let system_prompt = r#"You are a reasoning engine. Analyze the given memory and context to infer logical relationships.
Output JSON array with relations: [{"from_id": "...", "to_id": "...", "type": "IMPLIES|BECAUSE|CONTRADICTS|SUPPORTS", "strength": 0-100}]"#;

        let user_prompt = format!(
            "Memory ID: {}\nContext memories:\n{}",
            memory_id,
            context_memories.join("\n")
        );

        match llm.generate(system_prompt, &user_prompt, Some("json")).await {
            Ok((response, _metadata)) => {
                
                match serde_json::from_str::<Vec<serde_json::Value>>(&response) {
                    Ok(inferred) => {
                        let relations: Vec<ReasoningRelation> = inferred
                            .iter()
                            .filter_map(|r| {
                                Some(ReasoningRelation {
                                    relation_id: format!(
                                        "inferred_{}_{}",
                                        r.get("from_id")?.as_str()?,
                                        r.get("to_id")?.as_str()?
                                    ),
                                    from_memory_id: r.get("from_id")?.as_str()?.to_string(),
                                    to_memory_id: r.get("to_id")?.as_str()?.to_string(),
                                    to_memory_content: String::new(),
                                    relation_type: match r.get("type")?.as_str()? {
                                        "IMPLIES" => ReasoningType::Implies,
                                        "BECAUSE" => ReasoningType::Because,
                                        "CONTRADICTS" => ReasoningType::Contradicts,
                                        _ => ReasoningType::Supports,
                                    },
                                    strength: r.get("strength")?.as_i64()? as i32,
                                    reasoning_id: Some("llm_inferred".to_string()),
                                })
                            })
                            .collect();

                        debug!("LLM inferred {} relations", relations.len());
                        Ok(relations)
                    }
                    Err(e) => {
                        warn!("Failed to parse LLM response: {}", e);
                        Ok(Vec::new())
                    }
                }
            }
            Err(e) => {
                error!("LLM inference failed: {}", e);
                Err(ReasoningError::LlmError(e.to_string()))
            }
        }
    }

    
    fn build_reasoning_trail(&self, relations: &[ReasoningRelation]) -> String {
        if relations.is_empty() {
            return "No reasoning chain found.".to_string();
        }

        let mut trail = String::new();
        for (i, rel) in relations.iter().enumerate() {
            let arrow = match rel.relation_type {
                ReasoningType::Implies => "→",
                ReasoningType::Because => "←",
                ReasoningType::Contradicts => "⊗",
                ReasoningType::Supports => "↔",
            };

            if i > 0 {
                trail.push_str(" ");
            }
            trail.push_str(&format!(
                "[{}] {} [{}]",
                crate::safe_truncate(&rel.from_memory_id, 8),
                arrow,
                crate::safe_truncate(&rel.to_memory_id, 8)
            ));
        }

        trail
    }

    
    #[must_use]
    pub fn get_cache_stats(&self) -> CacheStats {
        use std::sync::atomic::Ordering;
        CacheStats {
            size: self.relation_cache.lock().len(),
            capacity: self.cache_size,
            is_warmed_up: self.is_warmed_up.load(Ordering::Relaxed),
        }
    }
}


#[derive(Debug, Clone)]
pub struct CacheStats {
    
    pub size: usize,
    
    pub capacity: usize,
    
    pub is_warmed_up: bool,
}


#[derive(Debug, thiserror::Error)]
pub enum ReasoningError {
    
    #[error("Database error: {0}")]
    Database(String),

    
    #[error("Invalid relation: {0}")]
    Invalid(String),

    
    #[error("LLM error: {0}")]
    LlmError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reasoning_type_edge_name() {
        assert_eq!(ReasoningType::Implies.edge_name(), "IMPLIES");
        assert_eq!(ReasoningType::Because.edge_name(), "BECAUSE");
        assert_eq!(ReasoningType::Contradicts.edge_name(), "CONTRADICTS");
        assert_eq!(ReasoningType::Supports.edge_name(), "SUPPORTS");
    }

    #[test]
    fn test_relation_creation() {
        let relation = ReasoningRelation {
            relation_id: "test".to_string(),
            from_memory_id: "mem_1".to_string(),
            to_memory_id: "mem_2".to_string(),
            relation_type: ReasoningType::Implies,
            strength: 80,
            reasoning_id: None,
        };

        assert_eq!(relation.strength, 80);
        assert_eq!(relation.relation_type, ReasoningType::Implies);
    }

    #[test]
    fn test_build_reasoning_trail() {
        let relations = vec![
            ReasoningRelation {
                relation_id: "r1".to_string(),
                from_memory_id: "mem_aaaa".to_string(),
                to_memory_id: "mem_bbbb".to_string(),
                relation_type: ReasoningType::Implies,
                strength: 90,
                reasoning_id: None,
            },
            ReasoningRelation {
                relation_id: "r2".to_string(),
                from_memory_id: "mem_bbbb".to_string(),
                to_memory_id: "mem_cccc".to_string(),
                relation_type: ReasoningType::Because,
                strength: 85,
                reasoning_id: None,
            },
        ];

        let client = HelixDB::new(None, None, None);
        let engine = ReasoningEngine::new(client, None, 100);
        let trail = engine.build_reasoning_trail(&relations);

        assert!(trail.contains("→"));
        assert!(trail.contains("←"));
    }
}
