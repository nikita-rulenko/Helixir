use tracing::{info, debug};

use super::helpers::safe_truncate;
use super::types::{SearchMemoryResult, ReasoningChainSearchResult, ToolingReasoningChain, ChainNode, ToolingError};
use super::ToolingManager;

impl ToolingManager {
    pub async fn search_reasoning_chain(
        &self,
        query: &str,
        user_id: &str,
        chain_mode: &str,
        max_depth: usize,
        limit: usize,
    ) -> Result<ReasoningChainSearchResult, ToolingError> {
        info!("Reasoning chain search: '{}...' mode={} depth={} limit={}",
            safe_truncate(query, 30), chain_mode, max_depth, limit);

        let query_embedding = self
            .embedder
            .generate(query, true)
            .await
            .map_err(|e| ToolingError::Embedding(e.to_string()))?;

        let seed_results = self
            .search_engine
            .search(query, &query_embedding, user_id, limit, "contextual", None, "personal")
            .await?;

        if seed_results.is_empty() {
            debug!("No seed memories found for query");
            return Ok(ReasoningChainSearchResult {
                chains: Vec::new(),
                total_memories: 0,
                deepest_chain: 0,
            });
        }

        let mut all_chains = Vec::new();
        let mut max_chain_depth = 0;
        let mut total_memories = 0;

        for seed in &seed_results {
            match self.reasoning_engine.get_chain(&seed.memory_id, chain_mode, max_depth).await {
                Ok(chain) => {
                    if !chain.relations.is_empty() {
                        let chain_depth = chain.depth;
                        max_chain_depth = max_chain_depth.max(chain_depth);
                        total_memories += chain.relations.len();

                        all_chains.push(ToolingReasoningChain {
                            seed: SearchMemoryResult {
                                memory_id: seed.memory_id.clone(),
                                content: seed.content.clone(),
                                score: seed.score as f64,
                                method: seed.method.clone(),
                                metadata: seed.metadata.clone(),
                                created_at: seed.created_at.clone(),
                            },
                            nodes: chain.relations.iter().map(|r| ChainNode {
                                memory_id: r.to_memory_id.clone(),
                                content: r.to_memory_content.clone(),
                                relation: r.relation_type.edge_name().to_string(),
                                depth: 0,
                            }).collect(),
                            chain_type: chain.chain_type.clone(),
                            reasoning_trail: chain.reasoning_trail.clone(),
                        });
                    }
                }
                Err(e) => {
                    debug!("Failed to get chain for {}: {}", seed.memory_id, e);
                }
            }
        }

        info!("Found {} chains, max_depth={}, total_memories={}",
            all_chains.len(), max_chain_depth, total_memories);

        Ok(ReasoningChainSearchResult {
            chains: all_chains,
            total_memories,
            deepest_chain: max_chain_depth,
        })
    }
}
