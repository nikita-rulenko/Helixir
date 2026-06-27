use tracing::{debug, info};

use super::ToolingManager;
use super::types::{
    ChainNode, ReasoningChainSearchResult, SearchMemoryResult, ToolingError, ToolingReasoningChain,
};
use crate::safe_truncate;

/// True if a `connect_memories` / `route` anchor argument is itself a memory id
/// (`mem_…` / `raw_…`) rather than a free-text query — in which case it anchors
/// directly instead of going through best-effort search (#59).
fn looks_like_memory_id(q: &str) -> bool {
    (q.starts_with("mem_") || q.starts_with("raw_")) && !q.chars().any(char::is_whitespace)
}

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
                .get_chain(
                    &seed.memory_id,
                    &seed.content,
                    chain_mode,
                    max_depth,
                    guidance,
                )
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
                                    // GH#23: expose the PEER (the other end of
                                    // the hop), not the to-endpoint — for
                                    // incoming edges `to` is the current node.
                                    memory_id: if r.peer_memory_id.is_empty() {
                                        r.to_memory_id.clone()
                                    } else {
                                        r.peer_memory_id.clone()
                                    },
                                    content: if r.peer_memory_id.is_empty() {
                                        r.to_memory_content.clone()
                                    } else {
                                        r.peer_memory_content.clone()
                                    },
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

    /// Elder-brain #14: "how is A related to B?" — bidirectional path
    /// discovery between two anchor queries over the typed reasoning graph.
    pub async fn connect_memories(
        &self,
        query_a: &str,
        query_b: &str,
        user_id: &str,
        max_depth: usize,
    ) -> Result<
        Option<crate::toolkit::mind_toolbox::search::smart_traversal_v2::ConnectionPath>,
        ToolingError,
    > {
        info!(
            "connect_memories: '{}' <-> '{}' (depth {})",
            safe_truncate(query_a, 30),
            safe_truncate(query_b, 30),
            max_depth
        );

        let mut seed_sets = Vec::with_capacity(2);
        for query in [query_a, query_b] {
            // #59: a query that IS a memory id anchors directly — no embedding,
            // no search. The search-based resolution is best-effort (top-3,
            // personal) and races the index on freshly-written memories, so a
            // caller that already knows the memory (a test, or an agent that
            // just stored it) can connect it deterministically by id.
            let seeds: Vec<(String, String)> = if looks_like_memory_id(query) {
                vec![(query.to_string(), String::new())]
            } else {
                let embedding = self
                    .embedder
                    .generate(query, true)
                    .await
                    .map_err(|e| ToolingError::Embedding(e.to_string()))?;
                self.search_engine
                    .search(
                        query, &embedding, user_id, 3, "full", None, None, "personal",
                    )
                    .await?
                    .into_iter()
                    .map(|r| (r.memory_id, r.content))
                    .collect()
            };
            seed_sets.push(seeds);
        }
        let seeds_b = seed_sets.pop().unwrap_or_default();
        let seeds_a = seed_sets.pop().unwrap_or_default();

        crate::toolkit::mind_toolbox::search::smart_traversal_v2::connect::connect(
            &self.db,
            &seeds_a,
            &seeds_b,
            max_depth,
            &self.config.retrieval.graph,
        )
        .await
        .map_err(|e| ToolingError::Database(e.to_string()))
    }

    /// Longest-chain context reconstruction (#47): from a `topic`, resolve seed
    /// memories, grow their reasoning ego-network, and return the single longest
    /// coherent reasoning thread — an ordered cause → effect → supersession
    /// narrative with edge types and cumulative confidence.
    pub async fn longest_chain(
        &self,
        topic: &str,
        user_id: &str,
        max_hops: usize,
    ) -> Result<
        Option<crate::toolkit::mind_toolbox::search::smart_traversal_v2::ChainNarrative>,
        ToolingError,
    > {
        info!(
            "longest_chain: topic '{}' (max_hops {})",
            safe_truncate(topic, 30),
            max_hops
        );

        let embedding = self
            .embedder
            .generate(topic, true)
            .await
            .map_err(|e| ToolingError::Embedding(e.to_string()))?;
        let seeds: Vec<(String, String)> = self
            .search_engine
            .search(topic, &embedding, user_id, 5, "full", None, None, "personal")
            .await?
            .into_iter()
            .map(|r| (r.memory_id, r.content))
            .collect();

        crate::toolkit::mind_toolbox::search::smart_traversal_v2::longest_chain::longest_chain(
            &self.db,
            &seeds,
            max_hops,
            &self.config.retrieval.graph,
        )
        .await
        .map_err(|e| ToolingError::Database(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::looks_like_memory_id;

    #[test]
    fn detects_memory_ids_but_not_queries() {
        assert!(looks_like_memory_id("mem_d48a6f6875ae"));
        assert!(looks_like_memory_id("raw_2b25ce44754d"));
        // free-text queries (the common case) must NOT be treated as ids
        assert!(!looks_like_memory_id("Rajasthan monsoon grain harvest"));
        assert!(!looks_like_memory_id("mem ory leak in the server")); // has a space
        assert!(!looks_like_memory_id("memory of a fact"));
        assert!(!looks_like_memory_id(""));
    }
}
