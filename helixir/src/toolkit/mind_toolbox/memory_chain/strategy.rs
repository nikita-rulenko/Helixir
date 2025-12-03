

use std::sync::Arc;
use std::collections::HashSet;
use tracing::{info, warn, error};
use crate::db::HelixClient;
use crate::llm::embeddings::EmbeddingGenerator;
use super::result::{ChainSearchResult, MemoryChain, ChainNode};
use super::config::MemoryChainConfig;


pub struct MemoryChainStrategy {
    client: Arc<HelixClient>,
    embedder: Arc<EmbeddingGenerator>,
    config: MemoryChainConfig,
}

impl MemoryChainStrategy {
    
    pub fn new(
        client: Arc<HelixClient>,
        embedder: Arc<EmbeddingGenerator>,
        config: Option<MemoryChainConfig>,
    ) -> Self {
        info!("MemoryChainStrategy initialized");
        Self {
            client,
            embedder,
            config: config.unwrap_or_default(),
        }
    }

    
    pub async fn search(
        &self,
        query: &str,
        _user_id: Option<&str>,
        limit: usize,
        config: Option<MemoryChainConfig>,
    ) -> ChainSearchResult {
        let query_preview = crate::safe_truncate(query, 50);
        info!("Chain search: '{}...' (limit={})", query_preview, limit);

        let config = config.unwrap_or_else(|| self.config.clone());

        
        let seeds = match self.vector_search(query, limit).await {
            Ok(s) => s,
            Err(e) => {
                error!("Vector search failed: {}", e);
                return ChainSearchResult::empty(query.to_string());
            }
        };

        if seeds.is_empty() {
            warn!("No seeds found for query: {}...", query_preview);
            return ChainSearchResult::empty(query.to_string());
        }

        info!("Found {} seed memories", seeds.len());

        
        let mut chains = Vec::new();
        for seed in &seeds {
            if let Some(chain) = self.build_chain_from_seed(seed, &config).await {
                chains.push(chain);
            }
        }

        
        chains.sort_by(|a, b| {
            let a_score = (a.nodes.len(), a.total_depth);
            let b_score = (b.nodes.len(), b.total_depth);
            b_score.cmp(&a_score)
        });

        let result = ChainSearchResult::new(query.to_string(), chains);

        info!(
            "Chain search complete: {} chains, {} total memories, max depth {}",
            result.total_chains, result.total_memories, result.deepest_chain
        );

        result
    }

    
    async fn vector_search(&self, query: &str, limit: usize) -> Result<Vec<serde_json::Value>, String> {
        let embedding = self.embedder.generate(query, true).await
            .map_err(|e| format!("Embedding failed: {}", e))?;

        #[derive(serde::Deserialize)]
        struct VectorResult {
            memories: Vec<serde_json::Value>,
            #[serde(default)]
            parent_memories: Vec<serde_json::Value>,
        }

        let params = serde_json::json!({
            "query_vector": embedding,
            "limit": limit,
        });

        let result: VectorResult = self.client
            .execute_query("smartVectorSearchWithChunks", &params)
            .await
            .map_err(|e| format!("Query failed: {}", e))?;

        
        let mut seen = HashSet::new();
        let mut memories = Vec::new();

        for mem in result.memories.into_iter().chain(result.parent_memories) {
            if let Some(id) = mem.get("memory_id").and_then(|v| v.as_str()) {
                if seen.insert(id.to_string()) {
                    memories.push(mem);
                }
            }
        }

        Ok(memories)
    }

    
    async fn build_chain_from_seed(
        &self,
        seed: &serde_json::Value,
        config: &MemoryChainConfig,
    ) -> Option<MemoryChain> {
        let seed_id = seed.get("memory_id")?.as_str()?;
        let seed_content = seed.get("content")?.as_str()?.to_string();

        let mut chain = MemoryChain::new(seed_id.to_string(), "mixed".to_string());

        
        chain.add_node(ChainNode {
            memory_id: seed_id.to_string(),
            content: seed_content,
            memory_type: seed.get("memory_type").and_then(|v| v.as_str()).map(String::from),
            depth: 0,
            relation_type: None,
        });

        
        let mut visited = HashSet::new();
        visited.insert(seed_id.to_string());

        self.expand_chain(&mut chain, seed_id, 1, config, &mut visited).await;

        if chain.nodes.len() > 1 {
            Some(chain)
        } else {
            None
        }
    }

    
    async fn expand_chain(
        &self,
        chain: &mut MemoryChain,
        node_id: &str,
        depth: u32,
        config: &MemoryChainConfig,
        visited: &mut HashSet<String>,
    ) {
        if depth > config.max_depth {
            return;
        }

        let params = serde_json::json!({"memory_id": node_id});

        #[derive(serde::Deserialize, Default)]
        struct Connections {
            #[serde(default)]
            implies_out: Vec<serde_json::Value>,
            #[serde(default)]
            implies_in: Vec<serde_json::Value>,
            #[serde(default)]
            because_out: Vec<serde_json::Value>,
            #[serde(default)]
            because_in: Vec<serde_json::Value>,
            #[serde(default)]
            contradicts_out: Vec<serde_json::Value>,
            #[serde(default)]
            contradicts_in: Vec<serde_json::Value>,
        }

        let connections: Connections = self.client
            .execute_query("getMemoryLogicalConnections", &params)
            .await
            .unwrap_or_default();

        let mut neighbors = Vec::new();

        if config.relation_types.contains(&"IMPLIES".to_string()) {
            neighbors.extend(connections.implies_out.into_iter().map(|m| (m, "IMPLIES")));
            neighbors.extend(connections.implies_in.into_iter().map(|m| (m, "IMPLIED_BY")));
        }

        if config.relation_types.contains(&"BECAUSE".to_string()) {
            neighbors.extend(connections.because_out.into_iter().map(|m| (m, "BECAUSE")));
            neighbors.extend(connections.because_in.into_iter().map(|m| (m, "CAUSED_BY")));
        }

        if config.include_contradictions && config.relation_types.contains(&"CONTRADICTS".to_string()) {
            neighbors.extend(connections.contradicts_out.into_iter().map(|m| (m, "CONTRADICTS")));
            neighbors.extend(connections.contradicts_in.into_iter().map(|m| (m, "CONTRADICTED_BY")));
        }

        for (mem, relation) in neighbors {
            if let Some(mem_id) = mem.get("memory_id").and_then(|v| v.as_str()) {
                if visited.insert(mem_id.to_string()) {
                    let content = mem.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();

                    chain.add_node(ChainNode {
                        memory_id: mem_id.to_string(),
                        content,
                        memory_type: mem.get("memory_type").and_then(|v| v.as_str()).map(String::from),
                        depth,
                        relation_type: Some(relation.to_string()),
                    });

                    chain.total_depth = chain.total_depth.max(depth);

                    Box::pin(self.expand_chain(chain, mem_id, depth + 1, config, visited)).await;
                }
            }
        }
    }

    
    pub async fn search_causal(&self, query: &str, user_id: Option<&str>, limit: usize) -> ChainSearchResult {
        self.search(query, user_id, limit, Some(MemoryChainConfig::causal_only())).await
    }

    
    pub async fn search_implications(&self, query: &str, user_id: Option<&str>, limit: usize) -> ChainSearchResult {
        self.search(query, user_id, limit, Some(MemoryChainConfig::implications_only())).await
    }

    
    pub async fn search_deep(&self, query: &str, user_id: Option<&str>, limit: usize) -> ChainSearchResult {
        self.search(query, user_id, limit, Some(MemoryChainConfig::deep_context())).await
    }
}
