//! Memory graph, concept, reasoning, and maintenance tools.
//! Long-term memory MCP tools.
//!
//! Covers the user-visible memory verbs: add, search (semantic + concept +
//! reasoning chain), list, update, graph, and the helper that finds
//! previously-timed-out FastThink commits.

use rmcp::{
    ErrorData as McpError, handler::server::wrapper::Parameters, model::*, tool, tool_router,
};
use serde_json::json;
use tracing::{info, warn};

use crate::mcp::params::*;
use crate::mcp::server::HelixirMcpServer;

#[tool_router(router = memory_graph_router, vis = "pub(super)")]
impl HelixirMcpServer {
    #[tool(
        description = "Replace the content of an EXISTING memory (you must pass its memory_id, e.g. from a search result); the embedding and graph relations are regenerated. Use to correct or refine a specific known fact. Note: this edits in place and Helixir never deletes — to retire an OUTDATED fact, prefer add_memory with the corrected statement and let the charter supersede the old one (history is preserved). Returns {updated: bool, memory_id}."
    )]
    async fn update_memory(
        &self,
        Parameters(params): Parameters<UpdateMemoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let id_preview: String = params.memory_id.chars().take(12).collect();
        info!("Updating memory: {}...", id_preview);
        let actor_id = self
            .actor_id(params.actor_id.as_deref(), &params.user_id)
            .await?;

        let result = self
            .client()
            .update_as(&actor_id, &params.memory_id, &params.new_content)
            .await
            .map_err(Self::convert_error)?;

        if result.updated {
            info!("Memory updated");
        } else {
            warn!("Memory update failed");
        }

        let json = Self::result_to_json(&result)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Return the user's knowledge graph as {nodes, edges}. Nodes are memories ({id, content, node_type}); edges are typed relations ({source, target, edge_type, weight}) where edge_type is BECAUSE/IMPLIES/SUPPORTS/CONTRADICTS. Pass memory_id to get the ego-network around one memory (radius = depth, default 2); omit it for the user's whole local graph. Use this to inspect structure — to WALK a reasoning chain use search_reasoning_chain, to find a PATH between two memories use connect_memories."
    )]
    async fn get_memory_graph(
        &self,
        Parameters(params): Parameters<GetMemoryGraphParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("Getting memory graph for user={}", params.user_id);
        let actor_id = self
            .actor_id(params.actor_id.as_deref(), &params.user_id)
            .await?;

        let result = self
            .client()
            .get_graph_as(
                &actor_id,
                &params.user_id,
                params.memory_id.as_deref(),
                params.depth.map(|d| d as usize),
            )
            .await
            .map_err(Self::convert_error)?;

        let json = Self::result_to_json(&result)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Semantic search restricted to ONE ontology type and/or tags — like search_memory but when you only want, say, the user's goals or preferences. Set concept_type to filter (one of skill/preference/goal/fact/opinion/experience/achievement/action; omit to search all types) and/or 'tags' (comma-separated). For unrestricted recall use search_memory. Returns [{memory_id, content, concept_score}]."
    )]
    async fn search_by_concept(
        &self,
        Parameters(params): Parameters<SearchByConceptParams>,
    ) -> Result<CallToolResult, McpError> {
        let query_preview: String = params.query.chars().take(30).collect();
        info!(
            "Concept search: '{}' type={:?}",
            query_preview, params.concept_type
        );
        let actor_id = self
            .actor_id(params.actor_id.as_deref(), &params.user_id)
            .await?;

        let results = self
            .client()
            .search_by_concept_as(
                &actor_id,
                &params.query,
                &params.user_id,
                params.concept_type.map(|c| c.as_str()),
                params.tags.as_deref(),
                params.mode.map(|m| m.as_str()),
                params.limit.map(|l| l as usize),
            )
            .await
            .map_err(Self::convert_error)?;

        info!("Found {} memories", results.len());

        let json = Self::result_to_json(&results)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Reconstruct chains of reasoning around a topic — the 'why / what-follows' tool, and Helixir's signature capability. It finds seed memories then walks typed reasoning edges (BECAUSE/IMPLIES/SUPPORTS/CONTRADICTS) to assemble cause->effect chains with a human-readable reasoning_trail. Use chain_mode 'causal' for 'why is X so', 'forward' for 'what does X lead to', 'both'/'deep' for full context. Can return a LARGE payload on a dense graph — keep max_depth (default 5) and limit modest. Returns {query, chains:[{seed, nodes, reasoning_trail}], total_memories, deepest_chain}."
    )]
    async fn search_reasoning_chain(
        &self,
        Parameters(params): Parameters<SearchReasoningChainParams>,
    ) -> Result<CallToolResult, McpError> {
        let chain_mode = params.chain_mode.map(|c| c.as_str()).unwrap_or("both");

        let query_preview: String = params.query.chars().take(30).collect();
        info!("Reasoning chain: '{}' mode={}", query_preview, chain_mode);
        let actor_id = self
            .actor_id(params.actor_id.as_deref(), &params.user_id)
            .await?;

        let result = self
            .client()
            .search_reasoning_chain_as(
                &actor_id,
                &params.query,
                &params.user_id,
                Some(chain_mode),
                params.max_depth.map(|d| d as usize),
                params.limit.map(|l| l as usize),
            )
            .await
            .map_err(Self::convert_error)?;

        info!("Found {} chains", result.chains.len());

        let json = Self::result_to_json(&result)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Discover how two concepts are related through the memory graph: bidirectional path search between anchors A and B. Each anchor may be a free-text query OR an exact memory_id (mem_… / raw_…) — pass an id to connect a memory you already know precisely, bypassing the search step. Returns the connecting chain with edge types (IMPLIES/BECAUSE/...) and cumulative confidence. The elder-brain primitive: sees connections that are several logical hops apart."
    )]
    async fn connect_memories(
        &self,
        Parameters(params): Parameters<ConnectMemoriesParams>,
    ) -> Result<CallToolResult, McpError> {
        info!(
            "Connect: '{}' <-> '{}'",
            params.query_a.chars().take(30).collect::<String>(),
            params.query_b.chars().take(30).collect::<String>()
        );
        let actor_id = self
            .actor_id(params.actor_id.as_deref(), &params.user_id)
            .await?;

        let result = self
            .client()
            .connect_memories_as(
                &actor_id,
                &params.query_a,
                &params.query_b,
                &params.user_id,
                params.max_depth.map(|d| d as usize),
            )
            .await
            .map_err(Self::convert_error)?;

        info!("Connection: found={} hops={}", result.found, result.hops);

        let json = Self::result_to_json(&result)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Find incomplete thoughts from previous sessions that timed out. Use at session start to continue unfinished research. Returns: [{memory_id, content, created_at}]"
    )]
    async fn search_incomplete_thoughts(
        &self,
        Parameters(params): Parameters<SearchIncompleteThoughtsParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("Searching for incomplete thoughts");

        let limit = params.limit.unwrap_or(5) as usize;
        let owner = params.user_id.as_deref().unwrap_or("");
        let actor_id = self.actor_id(params.actor_id.as_deref(), owner).await?;
        let client = self.client();

        let results = client
            .tooling()
            .search_by_tag("incomplete_thought", limit)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let memory_ids = results
            .iter()
            .map(|result| result.memory_id.clone())
            .collect::<Vec<_>>();
        let visible = client
            .rbac()
            .visible_memory_ids(&actor_id, &memory_ids)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let results = results
            .into_iter()
            .filter(|result| {
                visible
                    .as_ref()
                    .is_none_or(|allowed| allowed.contains(&result.memory_id))
            })
            .collect::<Vec<_>>();

        if results.is_empty() {
            let json = Self::result_to_json(json!({
                "found": 0,
                "message": "No incomplete thoughts found"
            }))?;
            return Ok(CallToolResult::success(vec![Content::text(json)]));
        }

        let json = Self::result_to_json(json!({
            "found": results.len(),
            "incomplete_thoughts": results.iter().map(|r| {
                json!({
                    "memory_id": r.memory_id,
                    "content": r.content,
                    "created_at": r.created_at
                })
            }).collect::<Vec<_>>()
        }))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}
