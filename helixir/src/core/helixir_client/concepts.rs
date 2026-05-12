//! Concept-driven search methods: `search_by_concept`, `search_reasoning_chain`.

use super::client::HelixirClient;
use super::error::HelixirClientError;
use super::types::{ChainNode, ReasoningChain, ReasoningChainResult, SearchResult};

impl HelixirClient {
    pub async fn search_by_concept(
        &self,
        query: &str,
        user_id: &str,
        concept_type: Option<&str>,
        tags: Option<&str>,
        mode: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<SearchResult>, HelixirClientError> {
        self.ensure_initialized().await?;

        let results = self
            .tooling_manager
            .search_by_concept(
                query,
                user_id,
                concept_type,
                tags,
                mode.unwrap_or("contextual"),
                limit.unwrap_or(10),
            )
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        Ok(results
            .into_iter()
            .map(|r| SearchResult {
                id: r.memory_id,
                content: r.content,
                score: r.score as f32,
                metadata: r.metadata,
                created_at: r.created_at,
            })
            .collect())
    }

    pub async fn search_reasoning_chain(
        &self,
        query: &str,
        user_id: &str,
        chain_mode: Option<&str>,
        max_depth: Option<usize>,
        limit: Option<usize>,
    ) -> Result<ReasoningChainResult, HelixirClientError> {
        self.ensure_initialized().await?;

        let result = self
            .tooling_manager
            .search_reasoning_chain(
                query,
                user_id,
                chain_mode.unwrap_or("both"),
                max_depth.unwrap_or(5),
                limit.unwrap_or(5),
            )
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        let chains = result
            .chains
            .into_iter()
            .map(|tc| ReasoningChain {
                seed: SearchResult {
                    id: tc.seed.memory_id,
                    content: tc.seed.content,
                    score: tc.seed.score as f32,
                    metadata: tc.seed.metadata,
                    created_at: tc.seed.created_at,
                },
                nodes: tc
                    .nodes
                    .into_iter()
                    .map(|n| ChainNode {
                        memory_id: n.memory_id,
                        content: n.content,
                        relation: n.relation,
                        depth: n.depth,
                    })
                    .collect(),
                chain_type: tc.chain_type,
                reasoning_trail: tc.reasoning_trail,
            })
            .collect();

        Ok(ReasoningChainResult {
            query: query.to_string(),
            chains,
            total_memories: result.total_memories,
            deepest_chain: result.deepest_chain,
        })
    }
}
