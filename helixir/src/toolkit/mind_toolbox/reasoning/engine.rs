use std::num::NonZeroUsize;
use std::sync::Arc;

use lru::LruCache;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

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

    /// Content of `to_memory_id`. INVARIANT: when populated, this MUST match
    /// the content of the memory pointed to by `to_memory_id` — both incoming
    /// and outgoing edges discovered during `get_chain` traversal. See #17.
    pub to_memory_content: String,

    /// Content of `from_memory_id`. Defaults to empty string when unknown
    /// (e.g. on a freshly persisted relation that never went through `get_chain`).
    /// See #17.
    #[serde(default)]
    pub from_memory_content: String,

    pub relation_type: ReasoningType,

    pub strength: i32,

    pub reasoning_id: Option<String>,
}

/// Pure projection helper: given a current node and the freshly discovered
/// neighbour, produce a `ReasoningRelation` whose `(from_memory_id, to_memory_id)`
/// physically reflects the edge direction in storage AND whose
/// `(*_memory_id, *_memory_content)` pairs stay aligned. This is the
/// canonical fix for #17.
#[must_use]
pub(super) fn project_relation(
    current_id: &str,
    current_content: &str,
    neighbor_id: &str,
    neighbor_content: &str,
    relation_type: ReasoningType,
    is_incoming: bool,
    strength: i32,
) -> ReasoningRelation {
    let (from_id, from_content, to_id, to_content) = if is_incoming {
        (neighbor_id, neighbor_content, current_id, current_content)
    } else {
        (current_id, current_content, neighbor_id, neighbor_content)
    };

    ReasoningRelation {
        relation_id: format!(
            "rel_{}_{}",
            crate::safe_truncate(from_id, 8),
            crate::safe_truncate(to_id, 8)
        ),
        from_memory_id: from_id.to_string(),
        to_memory_id: to_id.to_string(),
        to_memory_content: to_content.to_string(),
        from_memory_content: from_content.to_string(),
        relation_type,
        strength,
        reasoning_id: None,
    }
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

        // Reject self-referential reasoning edges. A memory cannot logically
        // IMPLIES / BECAUSE / CONTRADICTS / SUPPORTS itself; persisting such
        // an edge corrupts later chain traversal. See issue #16.
        if from_id == to_id {
            warn!(
                "Rejecting self-referential {} edge for memory {}",
                relation_type.edge_name(),
                crate::safe_truncate(from_id, 12)
            );
            return Err(ReasoningError::Invalid(format!(
                "self-referential {} edge on {}",
                relation_type.edge_name(),
                from_id
            )));
        }

        if self.edge_exists(from_id, to_id, relation_type).await {
            debug!(
                "Skipping duplicate {} edge: {} -> {}",
                relation_type.edge_name(),
                crate::safe_truncate(from_id, 12),
                crate::safe_truncate(to_id, 12)
            );
            return Ok(ReasoningRelation {
                relation_id: format!(
                    "rel_{}_{}",
                    crate::safe_truncate(from_id, 8),
                    crate::safe_truncate(to_id, 8)
                ),
                from_memory_id: from_id.to_string(),
                to_memory_id: to_id.to_string(),
                to_memory_content: String::new(),
                from_memory_content: String::new(),
                relation_type,
                strength,
                reasoning_id: reasoning_id.map(String::from),
            });
        }

        let relation = ReasoningRelation {
            relation_id: format!(
                "rel_{}_{}",
                crate::safe_truncate(from_id, 8),
                crate::safe_truncate(to_id, 8)
            ),
            from_memory_id: from_id.to_string(),
            to_memory_id: to_id.to_string(),
            to_memory_content: String::new(),
            from_memory_content: String::new(),
            relation_type,
            strength,
            reasoning_id: reasoning_id.map(String::from),
        };

        #[derive(Deserialize)]
        #[allow(dead_code)] // HelixDB edge-creation ack envelope.
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
        seed_content: &str,
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

        let is_deep = chain_type == "deep";
        let effective_max_depth = if is_deep { max_depth.max(8) } else { max_depth };

        let mut relations = Vec::new();
        let mut visited = std::collections::HashSet::new();
        // Frontier carries `(memory_id, content, depth)` so projection helpers
        // can pair `to_memory_id` with the matching `to_memory_content` regardless
        // of edge direction. See #17.
        let mut frontier: Vec<(String, String, usize)> =
            vec![(memory_id.to_string(), seed_content.to_string(), 0)];

        while let Some((current_id, current_content, current_depth)) = frontier.pop() {
            if current_depth >= effective_max_depth || visited.contains(&current_id) {
                continue;
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
                Err(e) => {
                    warn!(
                        "getMemoryLogicalConnections failed for {}: {} (depth={})",
                        crate::safe_truncate(&current_id, 16),
                        e,
                        current_depth
                    );
                    continue;
                }
            };

            let candidates: Vec<(MemoryNode, ReasoningType, bool)> = match chain_type {
                "causal" => {
                    let mut c = Vec::new();
                    for n in &result.because_in {
                        c.push((n.clone(), ReasoningType::Because, true));
                    }
                    for n in &result.because_out {
                        c.push((n.clone(), ReasoningType::Because, false));
                    }
                    for n in &result.implies_in {
                        c.push((n.clone(), ReasoningType::Implies, true));
                    }
                    c
                }
                "forward" => {
                    let mut c = Vec::new();
                    for n in &result.implies_out {
                        c.push((n.clone(), ReasoningType::Implies, false));
                    }
                    for n in &result.implies_in {
                        c.push((n.clone(), ReasoningType::Implies, true));
                    }
                    for n in &result.because_out {
                        c.push((n.clone(), ReasoningType::Because, false));
                    }
                    c
                }
                _ => {
                    let mut all = Vec::new();
                    for n in &result.implies_out {
                        all.push((n.clone(), ReasoningType::Implies, false));
                    }
                    for n in &result.implies_in {
                        all.push((n.clone(), ReasoningType::Implies, true));
                    }
                    for n in &result.because_out {
                        all.push((n.clone(), ReasoningType::Because, false));
                    }
                    for n in &result.because_in {
                        all.push((n.clone(), ReasoningType::Because, true));
                    }
                    for n in &result.contradicts_out {
                        all.push((n.clone(), ReasoningType::Contradicts, false));
                    }
                    for n in &result.contradicts_in {
                        all.push((n.clone(), ReasoningType::Contradicts, true));
                    }
                    for n in &result.relation_out {
                        all.push((n.clone(), ReasoningType::Supports, false));
                    }
                    for n in &result.relation_in {
                        all.push((n.clone(), ReasoningType::Supports, true));
                    }
                    all
                }
            };

            let unvisited: Vec<_> = candidates
                .into_iter()
                .filter(|(n, _, _)| !visited.contains(&n.memory_id))
                .collect();

            if unvisited.is_empty() {
                continue;
            }

            if is_deep {
                for (node, relation_type, is_incoming) in &unvisited {
                    relations.push(project_relation(
                        &current_id,
                        &current_content,
                        &node.memory_id,
                        &node.content,
                        *relation_type,
                        *is_incoming,
                        80,
                    ));

                    frontier.push((
                        node.memory_id.clone(),
                        node.content.clone(),
                        current_depth + 1,
                    ));
                }
            } else {
                let best = if unvisited.len() == 1 {
                    unvisited.into_iter().next()
                } else if let Some(llm) = &self.llm_provider {
                    let prompt = format!(
                        "Given current memory and {} connected memories, which ONE is most logically relevant?\n\nCurrent: {}\n\nOptions:\n{}\n\nRespond with just the number (1-{}).",
                        unvisited.len(),
                        &current_id[..current_id.len().min(50)],
                        unvisited
                            .iter()
                            .enumerate()
                            .map(|(i, (n, t, _))| format!(
                                "{}. [{}] {}",
                                i + 1,
                                t.edge_name(),
                                n.content.chars().take(100).collect::<String>()
                            ))
                            .collect::<Vec<_>>()
                            .join("\n"),
                        unvisited.len()
                    );

                    match llm
                        .generate(
                            "You are a reasoning assistant. Pick the most relevant connection.",
                            &prompt,
                            None,
                        )
                        .await
                    {
                        Ok((response, _)) => {
                            let choice: usize = response.trim().parse().unwrap_or(1);
                            unvisited.into_iter().nth(choice.saturating_sub(1))
                        }
                        Err(e) => {
                            warn!("LLM chain selection failed: {}", e);
                            unvisited.into_iter().next()
                        }
                    }
                } else {
                    unvisited.into_iter().next()
                };

                if let Some((node, relation_type, is_incoming)) = best {
                    relations.push(project_relation(
                        &current_id,
                        &current_content,
                        &node.memory_id,
                        &node.content,
                        relation_type,
                        is_incoming,
                        80,
                    ));

                    frontier.push((
                        node.memory_id.clone(),
                        node.content.clone(),
                        current_depth + 1,
                    ));
                }
            }
        }

        let max_depth_reached = relations.iter().count();
        let reasoning_trail = self.build_reasoning_trail(&relations);

        debug!(
            "Chain traversal for {}: type={}, relations={}, visited={}",
            crate::safe_truncate(memory_id, 12),
            chain_type,
            relations.len(),
            visited.len()
        );

        Ok(ReasoningChain {
            seed_memory_id: memory_id.to_string(),
            relations,
            chain_type: chain_type.to_string(),
            depth: max_depth_reached,
            reasoning_trail,
        })
    }

    pub async fn infer_relations(
        &self,
        new_memory_id: &str,
        new_memory_content: &str,
        similar_memories: &[(String, String)],
    ) -> Result<Vec<ReasoningRelation>, ReasoningError> {
        let Some(ref llm) = self.llm_provider else {
            return Ok(Vec::new());
        };

        if similar_memories.is_empty() {
            return Ok(Vec::new());
        }

        let system_prompt = r#"You are a reasoning engine that finds logical connections between memories. You MUST find at least one relationship if the memories share ANY topic, entity, or context.

Output a JSON array. Each element:
{"existing_index": 0, "type": "IMPLIES|BECAUSE|CONTRADICTS|SUPPORTS", "strength": 0-100}

Relation types:
- SUPPORTS: they share the same topic, reinforce each other, or provide evidence for the same conclusion (MOST COMMON — use when in doubt)
- IMPLIES: one logically leads to or suggests the other
- BECAUSE: one is a cause/reason for the other
- CONTRADICTS: they conflict or are incompatible

Rules:
- If both memories mention the same project, person, technology, or concept → at minimum SUPPORTS (strength 50-70)
- If one memory is a consequence of another → IMPLIES (strength 60-90)
- If one memory explains why another is true → BECAUSE (strength 60-90)
- Include relations with strength >= 40
- Output ONLY a valid JSON array, no markdown, no explanation
- If truly no connection exists (completely unrelated topics), output []"#;

        let context_str: String = similar_memories
            .iter()
            .enumerate()
            .map(|(i, (_, content))| format!("[{}] {}", i, content))
            .collect::<Vec<_>>()
            .join("\n");

        let user_prompt = format!(
            "NEW memory: {}\n\nEXISTING memories:\n{}",
            new_memory_content, context_str
        );

        let parse_relations = |response: &str| -> Vec<ReasoningRelation> {
            let parsed = serde_json::from_str::<Vec<serde_json::Value>>(response).or_else(|_| {
                if let Ok(obj) = serde_json::from_str::<serde_json::Value>(response) {
                    if let Some(arr) = obj
                        .get("relations")
                        .or_else(|| obj.get("results"))
                        .or_else(|| obj.get("data"))
                    {
                        if let Some(arr) = arr.as_array() {
                            return Ok(arr.clone());
                        }
                    }
                }
                if let Some(start) = response.find('[') {
                    if let Some(end) = response.rfind(']') {
                        return serde_json::from_str(&response[start..=end]);
                    }
                }
                Err(serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "no array",
                )))
            });

            match parsed {
                Ok(inferred) => inferred
                    .iter()
                    .filter_map(|r| {
                        let idx = r.get("existing_index")?.as_u64()? as usize;
                        let (target_id, target_content) = similar_memories.get(idx)?;
                        Some(ReasoningRelation {
                            relation_id: format!(
                                "inferred_{}_{}",
                                crate::safe_truncate(new_memory_id, 8),
                                crate::safe_truncate(target_id, 8)
                            ),
                            from_memory_id: new_memory_id.to_string(),
                            to_memory_id: target_id.clone(),
                            to_memory_content: target_content.clone(),
                            from_memory_content: new_memory_content.to_string(),
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
                    .collect(),
                Err(_) => Vec::new(),
            }
        };

        match llm
            .generate(system_prompt, &user_prompt, Some("json_object"))
            .await
        {
            Ok((response, _metadata)) => {
                info!(
                    "infer_relations LLM response ({}b): {}",
                    response.len(),
                    &response.chars().take(200).collect::<String>()
                );
                let relations = parse_relations(&response);
                if !relations.is_empty() {
                    info!(
                        "LLM inferred {} relations for {}",
                        relations.len(),
                        crate::safe_truncate(new_memory_id, 12)
                    );
                    return Ok(relations);
                }

                warn!("First infer_relations attempt returned 0 relations, retrying");
                let retry_prompt = format!(
                    "{}\n\nIMPORTANT: Output ONLY a valid JSON array. No markdown, no explanation. Example: [{{\"existing_index\":0,\"type\":\"SUPPORTS\",\"strength\":75}}]",
                    user_prompt
                );
                match llm
                    .generate(system_prompt, &retry_prompt, Some("json_object"))
                    .await
                {
                    Ok((retry_response, _)) => {
                        let retry_relations = parse_relations(&retry_response);
                        debug!("LLM inferred {} relations (retry)", retry_relations.len());
                        Ok(retry_relations)
                    }
                    Err(e) => {
                        warn!("LLM inference retry failed: {}", e);
                        Ok(Vec::new())
                    }
                }
            }
            Err(e) => {
                warn!("LLM inference failed (non-critical): {}", e);
                Ok(Vec::new())
            }
        }
    }

    async fn edge_exists(&self, from_id: &str, to_id: &str, relation_type: ReasoningType) -> bool {
        #[derive(Deserialize)]
        struct ConnectionsResult {
            #[serde(default)]
            implies_out: Vec<MemNode>,
            #[serde(default)]
            because_out: Vec<MemNode>,
            #[serde(default)]
            contradicts_out: Vec<MemNode>,
            #[serde(default)]
            relation_out: Vec<MemNode>,
        }

        #[derive(Deserialize)]
        struct MemNode {
            #[serde(default)]
            memory_id: String,
        }

        let result = match self
            .client
            .execute_query::<ConnectionsResult, _>(
                "getMemoryLogicalConnections",
                &serde_json::json!({"memory_id": from_id}),
            )
            .await
        {
            Ok(r) => r,
            Err(_) => return false,
        };

        let targets = match relation_type {
            ReasoningType::Implies => &result.implies_out,
            ReasoningType::Because => &result.because_out,
            ReasoningType::Contradicts => &result.contradicts_out,
            ReasoningType::Supports => &result.relation_out,
        };

        targets.iter().any(|n| n.memory_id == to_id)
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
                trail.push(' ');
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
            to_memory_content: "test content".to_string(),
            from_memory_content: String::new(),
            relation_type: ReasoningType::Implies,
            strength: 80,
            reasoning_id: None,
        };

        assert_eq!(relation.strength, 80);
        assert_eq!(relation.relation_type, ReasoningType::Implies);
    }

    #[test]
    fn project_relation_outgoing_pairs_id_with_content() {
        // Outgoing edge (current → neighbor): #17 invariant.
        let rel = project_relation(
            "mem_current",
            "current content",
            "mem_neighbor",
            "neighbor content",
            ReasoningType::Implies,
            false,
            80,
        );

        assert_eq!(rel.from_memory_id, "mem_current");
        assert_eq!(rel.from_memory_content, "current content");
        assert_eq!(rel.to_memory_id, "mem_neighbor");
        assert_eq!(rel.to_memory_content, "neighbor content");
    }

    #[test]
    fn project_relation_incoming_pairs_id_with_content() {
        // Incoming edge (neighbor → current): #17 specifically targeted this case
        // where `to_memory_content` used to leak the neighbour's content.
        let rel = project_relation(
            "mem_current",
            "current content",
            "mem_neighbor",
            "neighbor content",
            ReasoningType::Because,
            true,
            80,
        );

        assert_eq!(rel.from_memory_id, "mem_neighbor");
        assert_eq!(rel.from_memory_content, "neighbor content");
        assert_eq!(rel.to_memory_id, "mem_current");
        assert_eq!(rel.to_memory_content, "current content");
    }

    #[tokio::test]
    async fn add_relation_rejects_self_loop() {
        use std::sync::Arc;
        let client = Arc::new(crate::db::HelixClient::new("127.0.0.1", 1).unwrap());
        let engine = ReasoningEngine::new(client, None, 16);

        // Guard rejects self-loops before any DB roundtrip; see issue #16.
        for rt in [
            ReasoningType::Implies,
            ReasoningType::Because,
            ReasoningType::Contradicts,
            ReasoningType::Supports,
        ] {
            let result = engine.add_relation("mem_x", "mem_x", rt, 80, None).await;
            assert!(
                matches!(result, Err(ReasoningError::Invalid(_))),
                "self-loop must be rejected for {:?}",
                rt
            );
        }
    }

    #[test]
    fn test_build_reasoning_trail() {
        let relations = vec![
            ReasoningRelation {
                relation_id: "r1".to_string(),
                from_memory_id: "mem_aaaa".to_string(),
                to_memory_id: "mem_bbbb".to_string(),
                to_memory_content: "test content".to_string(),
                from_memory_content: String::new(),
                relation_type: ReasoningType::Implies,
                strength: 90,
                reasoning_id: None,
            },
            ReasoningRelation {
                relation_id: "r2".to_string(),
                from_memory_id: "mem_bbbb".to_string(),
                to_memory_id: "mem_cccc".to_string(),
                to_memory_content: "test content".to_string(),
                from_memory_content: String::new(),
                relation_type: ReasoningType::Because,
                strength: 85,
                reasoning_id: None,
            },
        ];

        use std::sync::Arc;
        let client = Arc::new(crate::db::HelixClient::new("localhost", 6969).unwrap());
        let engine = ReasoningEngine::new(client, None, 100);
        let trail = engine.build_reasoning_trail(&relations);

        assert!(trail.contains("→"));
        assert!(trail.contains("←"));
    }
}
