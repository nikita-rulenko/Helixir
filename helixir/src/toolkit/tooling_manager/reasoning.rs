use tracing::{debug, info};

use super::ToolingManager;
use crate::safe_truncate;
use super::types::{
    ChainNode, ReasoningChainSearchResult, SearchMemoryResult, ToolingError, ToolingReasoningChain,
};

impl ToolingManager {
    pub async fn search_reasoning_chain(
        &self,
        query: &str,
        user_id: &str,
        chain_mode: &str,
        max_depth: usize,
        limit: usize,
    ) -> Result<ReasoningChainSearchResult, ToolingError> {
        info!(
            "Reasoning chain search: '{}...' mode={} depth={} limit={}",
            safe_truncate(query, 30),
            chain_mode,
            max_depth,
            limit
        );

        let query_embedding = self
            .embedder
            .generate(query, true)
            .await
            .map_err(|e| ToolingError::Embedding(e.to_string()))?;

        let mut seed_results = self
            .search_engine
            .search(
                query,
                &query_embedding,
                user_id,
                limit,
                "contextual",
                None,
                "personal",
            )
            .await?;

        // algo_opt R3: a corpus older than the contextual window (30d) used to
        // make every chain query return empty. Widen to `full` before giving up.
        if seed_results.is_empty()
            && crate::core::RetrievalProfile::cached().embedding_guided_chains()
        {
            debug!("No contextual seeds; widening seed search to mode=full");
            seed_results = self
                .search_engine
                .search(
                    query,
                    &query_embedding,
                    user_id,
                    limit,
                    "full",
                    None,
                    "personal",
                )
                .await?;
        }

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

        // algo_opt R3: hand the query embedding to the chain walker so hop
        // selection runs on cosine similarity instead of an LLM call per hop.
        let guided = crate::core::RetrievalProfile::cached().embedding_guided_chains();

        for seed in &seed_results {
            let guidance = guided.then(|| crate::toolkit::mind_toolbox::reasoning::ChainGuidance {
                query_embedding: &query_embedding,
                embedder: &self.embedder,
            });
            match self
                .reasoning_engine
                .get_chain(&seed.memory_id, &seed.content, chain_mode, max_depth, guidance)
                .await
            {
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
                            nodes: chain
                                .relations
                                .iter()
                                .map(|r| ChainNode {
                                    memory_id: r.to_memory_id.clone(),
                                    content: r.to_memory_content.clone(),
                                    relation: r.relation_type.edge_name().to_string(),
                                    depth: 0,
                                })
                                .collect(),
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

        info!(
            "Found {} chains, max_depth={}, total_memories={}",
            all_chains.len(),
            max_chain_depth,
            total_memories
        );

        Ok(ReasoningChainSearchResult {
            chains: all_chains,
            total_memories,
            deepest_chain: max_chain_depth,
        })
    }
}
