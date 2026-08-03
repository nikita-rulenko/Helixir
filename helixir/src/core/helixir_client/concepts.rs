//! Concept-driven search methods: `search_by_concept`, `search_reasoning_chain`.

use super::client::HelixirClient;
use super::error::HelixirClientError;
use super::types::{
    ChainNode, ConnectMemoriesResult, ConnectionEdge, ConnectionNode, ReasoningChain,
    ReasoningChainResult, SearchResult,
};

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
        self.search_by_concept_as(user_id, query, user_id, concept_type, tags, mode, limit)
            .await
    }

    // SAFETY: the actor/owner split is explicit at this public boundary; the
    // remaining optional filters mirror the established search API.
    #[allow(clippy::too_many_arguments)]
    pub async fn search_by_concept_as(
        &self,
        actor_id: &str,
        query: &str,
        owner_id: &str,
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
                owner_id,
                concept_type,
                tags,
                mode.unwrap_or("contextual"),
                limit.unwrap_or(10),
            )
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        let memory_ids = results
            .iter()
            .map(|result| result.memory_id.clone())
            .collect::<Vec<_>>();
        let visible = self
            .rbac()
            .visible_memory_ids(actor_id, &memory_ids)
            .await
            .map_err(|e| HelixirClientError::Operation(e.to_string()))?;
        let results = results
            .into_iter()
            .filter(|result| {
                visible
                    .as_ref()
                    .map_or(true, |allowed| allowed.contains(&result.memory_id))
            })
            .collect::<Vec<_>>();

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
        self.search_reasoning_chain_as(user_id, query, user_id, chain_mode, max_depth, limit)
            .await
    }

    pub async fn search_reasoning_chain_as(
        &self,
        actor_id: &str,
        query: &str,
        owner_id: &str,
        chain_mode: Option<&str>,
        max_depth: Option<usize>,
        limit: Option<usize>,
    ) -> Result<ReasoningChainResult, HelixirClientError> {
        self.ensure_initialized().await?;

        let result = self
            .tooling_manager
            .search_reasoning_chain(
                query,
                owner_id,
                chain_mode.unwrap_or("both"),
                max_depth.unwrap_or(5),
                limit.unwrap_or(5),
            )
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        let memory_ids = result
            .chains
            .iter()
            .flat_map(|chain| {
                std::iter::once(chain.seed.memory_id.clone())
                    .chain(chain.nodes.iter().map(|node| node.memory_id.clone()))
            })
            .collect::<Vec<_>>();
        let visible = self
            .rbac()
            .visible_memory_ids(actor_id, &memory_ids)
            .await
            .map_err(|e| HelixirClientError::Operation(e.to_string()))?;
        let restricted = visible.is_some();
        let mut chains = Vec::with_capacity(result.chains.len());
        for tc in result.chains {
            if let Some(allowed) = &visible {
                if !allowed.contains(&tc.seed.memory_id) {
                    continue;
                }
                let original_nodes = tc.nodes.len();
                let visible_nodes = tc
                    .nodes
                    .into_iter()
                    .filter(|node| allowed.contains(&node.memory_id))
                    .map(|node| ChainNode {
                        memory_id: node.memory_id,
                        content: node.content,
                        relation: node.relation,
                        depth: node.depth,
                    })
                    .collect::<Vec<_>>();
                let filtered_nodes = visible_nodes.len() != original_nodes;
                chains.push(ReasoningChain {
                    seed: SearchResult {
                        id: tc.seed.memory_id,
                        content: tc.seed.content,
                        score: tc.seed.score as f32,
                        metadata: tc.seed.metadata,
                        created_at: tc.seed.created_at,
                    },
                    nodes: visible_nodes,
                    chain_type: tc.chain_type,
                    // A trail is generated from all relations.  Do not return
                    // it when any node was filtered, because it can contain
                    // content from an inaccessible group.
                    reasoning_trail: if filtered_nodes {
                        String::new()
                    } else {
                        tc.reasoning_trail
                    },
                });
            } else {
                chains.push(ReasoningChain {
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
                });
            }
        }

        let (total_memories, deepest_chain) = if restricted {
            let total = chains.iter().map(|chain| chain.nodes.len()).sum();
            let deepest = chains
                .iter()
                .map(|chain| chain.nodes.len())
                .max()
                .unwrap_or_default();
            (total, deepest)
        } else {
            (result.total_memories, result.deepest_chain)
        };

        Ok(ReasoningChainResult {
            query: query.to_string(),
            chains,
            total_memories,
            deepest_chain,
        })
    }

    /// "How is A related to B?" — bidirectional path discovery between two
    /// anchor queries (elder-brain primitive).
    pub async fn connect_memories(
        &self,
        query_a: &str,
        query_b: &str,
        user_id: &str,
        max_depth: Option<usize>,
    ) -> Result<ConnectMemoriesResult, HelixirClientError> {
        self.connect_memories_as(user_id, query_a, query_b, user_id, max_depth)
            .await
    }

    pub async fn connect_memories_as(
        &self,
        actor_id: &str,
        query_a: &str,
        query_b: &str,
        owner_id: &str,
        max_depth: Option<usize>,
    ) -> Result<ConnectMemoriesResult, HelixirClientError> {
        self.ensure_initialized().await?;

        let path = self
            .tooling_manager
            .connect_memories(query_a, query_b, owner_id, max_depth.unwrap_or(4))
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        let path = if let Some(path) = path {
            let memory_ids = path
                .nodes
                .iter()
                .map(|node| node.memory_id.clone())
                .collect::<Vec<_>>();
            let visible = self
                .rbac()
                .visible_memory_ids(actor_id, &memory_ids)
                .await
                .map_err(|e| HelixirClientError::Operation(e.to_string()))?;
            visible
                .as_ref()
                .map_or(true, |allowed| {
                    memory_ids.iter().all(|id| allowed.contains(id))
                })
                .then_some(path)
        } else {
            None
        };

        Ok(match path {
            Some(p) => ConnectMemoriesResult {
                found: true,
                shared_seed: p.shared_seed,
                hops: p.hops,
                confidence: p.confidence,
                nodes: p
                    .nodes
                    .into_iter()
                    .map(|n| ConnectionNode {
                        memory_id: n.memory_id,
                        content: n.content,
                    })
                    .collect(),
                edges: p
                    .edges
                    .into_iter()
                    .map(|e| ConnectionEdge {
                        edge_type: e.edge_type,
                        weight: e.weight,
                    })
                    .collect(),
            },
            None => ConnectMemoriesResult {
                found: false,
                shared_seed: false,
                hops: 0,
                confidence: 0.0,
                nodes: vec![],
                edges: vec![],
            },
        })
    }

    /// Longest-chain context reconstruction (#47): the longest coherent
    /// reasoning thread running through `topic` — an ordered cause → effect →
    /// supersession narrative with edge types and cumulative confidence.
    pub async fn longest_chain(
        &self,
        topic: &str,
        user_id: &str,
        max_hops: usize,
    ) -> Result<
        Option<crate::toolkit::mind_toolbox::search::smart_traversal::ChainNarrative>,
        HelixirClientError,
    > {
        self.longest_chain_as(user_id, topic, user_id, max_hops)
            .await
    }

    /// Longest-chain context reconstruction with explicit actor/owner RBAC.
    pub async fn longest_chain_as(
        &self,
        actor_id: &str,
        topic: &str,
        owner_id: &str,
        max_hops: usize,
    ) -> Result<
        Option<crate::toolkit::mind_toolbox::search::smart_traversal::ChainNarrative>,
        HelixirClientError,
    > {
        self.ensure_initialized().await?;
        let narrative = self
            .tooling_manager
            .longest_chain(topic, owner_id, max_hops)
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        let Some(narrative) = narrative else {
            return Ok(None);
        };
        let memory_ids = narrative
            .steps
            .iter()
            .map(|step| step.memory_id.clone())
            .collect::<Vec<_>>();
        let visible = self
            .rbac()
            .visible_memory_ids(actor_id, &memory_ids)
            .await
            .map_err(|e| HelixirClientError::Operation(e.to_string()))?;
        Ok(visible
            .as_ref()
            .map_or(true, |allowed| {
                memory_ids.iter().all(|id| allowed.contains(id))
            })
            .then_some(narrative))
    }
}
