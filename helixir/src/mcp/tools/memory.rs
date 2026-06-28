//! Long-term memory MCP tools.
//!
//! Covers the user-visible memory verbs: add, search (semantic + concept +
//! reasoning chain), list, update, graph, and the helper that finds
//! previously-timed-out FastThink commits.

use rmcp::{
    ErrorData as McpError, handler::server::wrapper::Parameters, model::*, tool, tool_router,
};
use serde_json::json;
use tracing::{debug, info, warn};

use crate::mcp::params::*;
use crate::mcp::server::{HelixirMcpServer, is_empty_user_graph_error};

#[tool_router(router = memory_router, vis = "pub(super)")]
impl HelixirMcpServer {
    #[tool(
        description = "Store something in long-term memory. Pass raw natural-language text; an LLM extracts it into atomic typed facts (max 15 per call), embeds them, and links them into the reasoning graph. Use this whenever the user states a fact, decision, preference, or outcome worth remembering across sessions; for >15 facts, split across calls. \
        \nReturns one of two shapes. (1) Synchronous: {memories_added, memory_ids, deduped, entities, relations, chunks_created, stats}. 'deduped' holds existing memory_ids this input was already-known-and-linked-to (not newly stored) — so memories_added=0 with a non-empty deduped means 'already saved', NOT a failure. (2) Buffered (when the server runs the ingest buffer): {pending_id, queued:true, status:'pending', pending_outcomes} — the write is processing in the background; poll get_add_status(pending_id) for the result, or read pending_outcomes (results of EARLIER buffered adds delivered opportunistically). \
        \nIMPORTANT: if the result contains a non-empty needs_clarification array, the memory charter refused to silently resolve a conflict (e.g. a reversed preference). Read each entry and ask the user its suggested_question (or apply a standing rule), then add the answer as a new memory — do not ignore it."
    )]
    async fn add_memory(
        &self,
        Parameters(params): Parameters<AddMemoryParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("🧠 Adding memory for user={}", params.user_id);

        // Ingest buffer (#25): when HELIXIR_INGEST_BUFFER=1, persist the raw
        // input and return a pending_id instantly; a serial worker processes
        // it and posts the result to the outbox (check_inbox). Synchronous
        // path (below) stays the default — backward compatible.
        if crate::toolkit::tooling_manager::ingest_buffer::buffer_enabled() {
            let enq = self
                .client
                .add_buffered(
                    &params.message,
                    &params.user_id,
                    params.agent_id.as_deref(),
                    None,
                )
                .await
                .map_err(Self::convert_error)?;
            info!("📥 Queued {} for background processing", enq.pending_id);
            // Opportunistic outbox delivery: ride prior write outcomes back on
            // this ack so the agent learns them without polling or check_inbox.
            let outcomes = self
                .client
                .drain_notices(&params.user_id, 20)
                .await
                .unwrap_or_default();
            let json = Self::result_to_json(&serde_json::json!({
                "pending_id": enq.pending_id,
                "status": enq.status,
                "queued": enq.queued,
                "pending_outcomes": outcomes,
            }))?;
            return Ok(CallToolResult::success(vec![Content::text(json)]));
        }

        let result = self
            .client
            .add(
                &params.message,
                &params.user_id,
                params.agent_id.as_deref(),
                None,
            )
            .await
            .map_err(Self::convert_error)?;

        info!(
            "✅ Added {} memories ({} chunks)",
            result.memories_added, result.chunks_created
        );

        if crate::toolkit::tooling_manager::ingest_buffer::buffer_enabled() {
            let outcomes = self
                .client
                .drain_notices(&params.user_id, 20)
                .await
                .unwrap_or_default();
            if !outcomes.is_empty() {
                let mut json = Self::result_to_value(&result)?;
                json["pending_outcomes"] = serde_json::to_value(&outcomes).unwrap_or_default();
                return Ok(CallToolResult::success(vec![Content::text(
                    json.to_string(),
                )]));
            }
        }

        let json = Self::result_to_json(&result)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Check the status of a buffered add_memory by its pending_id. Returns {status: pending|processing|done|failed|not_found, result?, error?}. Optional — outcomes are also delivered opportunistically as pending_outcomes on your next add_memory, so polling is not required."
    )]
    async fn get_add_status(
        &self,
        Parameters(params): Parameters<GetAddStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        let status = self
            .client
            .add_status(&params.pending_id)
            .await
            .map_err(Self::convert_error)?;
        let json = Self::result_to_json(&status)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Recall memories by meaning — the DEFAULT retrieval tool (hybrid dense + keyword + graph, no LLM call). Use it to answer 'what do I know about X'. Pick a sibling instead when: you want the WHY behind something → search_reasoning_chain; to bridge two specific concepts → connect_memories; to filter by ontology type/tags → search_by_concept; to dump everything for a user → list_memories. 'mode' sets recall breadth (recent ~4h / contextual ~30d default / deep ~90d / full = whole store; use full if a query you expect to match returns empty). 'scope' defaults to personal; collective/all need the collective tier and are downgraded to personal otherwise. Returns ranked [{memory_id, content, score, metadata}] where metadata carries provenance (origin, edge, ppr, cosine)."
    )]
    async fn search_memory(
        &self,
        Parameters(params): Parameters<SearchMemoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let mode = params
            .mode
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| self.client.config().default_search_mode.clone());
        let limit = params.limit.map(|l| l as usize);
        // Default scope is intentionally personal (GH #40): collective memory
        // stays hidden unless explicitly requested, so weak models aren't
        // flooded with other users' facts. Not a config knob — a safety default.
        let requested_scope = params.scope.map(|s| s.as_str()).unwrap_or("personal");
        // Solo mode answers only from the user's own memory — a collective/all
        // request is downgraded to personal rather than leaking other users'.
        let scope = if self.client.config().mode.collective_enabled() {
            requested_scope
        } else {
            "personal"
        };

        let query_preview: String = params.query.chars().take(50).collect();
        info!(
            "🔍 Searching: '{}' [mode={}, limit={:?}, scope={}]",
            query_preview, mode, limit, scope
        );

        let results = self
            .client
            .search(
                &params.query,
                &params.user_id,
                limit,
                Some(&mode),
                params.temporal_days,
                params.graph_depth.map(|d| d as usize),
                Some(scope),
            )
            .await
            .map_err(Self::convert_error)?;

        info!("✅ Found {} memories", results.len());

        let json = Self::result_to_json(&results)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Dump a user's memories in bulk (newest first), with NO ranking by relevance — use it for counting, auditing, or seeing everything; for 'what's relevant to X' use search_memory instead. Optionally restrict to one ontology type via memory_type. Capped by 'limit' (default 100) and truncated on large stores, so it is not a substitute for search. Returns [{memory_id, content, memory_type, created_at, importance, certainty}]."
    )]
    async fn list_memories(
        &self,
        Parameters(params): Parameters<ListMemoriesParams>,
    ) -> Result<CallToolResult, McpError> {
        let limit = params.limit.unwrap_or(100) as i64;
        info!(
            "📋 Listing memories for user={}, limit={}",
            params.user_id, limit
        );

        #[derive(serde::Deserialize)]
        struct MemoriesResponse {
            #[serde(default)]
            memories: Vec<serde_json::Value>,
        }

        // HelixDB raises `Graph error: No value found` (also serialised with
        // the code `GRAPH_ERROR`) when the user has zero outgoing
        // `HAS_MEMORY` edges — i.e. either the user node is brand new or it
        // doesn't exist yet. Both states are semantically equivalent to "no
        // memories", so we map them to an empty Vec instead of bubbling an
        // MCP error to the caller. See issue #19.
        let result: MemoriesResponse = match self
            .client
            .db()
            .execute_query(
                "getUserMemories",
                &serde_json::json!({
                    "user_id": params.user_id,
                    "limit": limit
                }),
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let msg = e.to_string();
                if is_empty_user_graph_error(&msg) {
                    debug!(
                        "list_memories: user '{}' has no memories yet (HelixDB returned '{}')",
                        params.user_id, msg
                    );
                    MemoriesResponse {
                        memories: Vec::new(),
                    }
                } else {
                    return Err(McpError::internal_error(msg, None));
                }
            }
        };

        let mut memories = result.memories;

        if let Some(mem_type) = params.memory_type {
            memories.retain(|m| {
                m.get("memory_type")
                    .and_then(|v| v.as_str())
                    .map(|t| t == mem_type.as_str())
                    .unwrap_or(false)
            });
        }

        info!("📋 Listed {} memories", memories.len());
        let json = serde_json::to_string_pretty(&memories)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Replace the content of an EXISTING memory (you must pass its memory_id, e.g. from a search result); the embedding and graph relations are regenerated. Use to correct or refine a specific known fact. Note: this edits in place and Helixir never deletes — to retire an OUTDATED fact, prefer add_memory with the corrected statement and let the charter supersede the old one (history is preserved). Returns {updated: bool, memory_id}."
    )]
    async fn update_memory(
        &self,
        Parameters(params): Parameters<UpdateMemoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let id_preview: String = params.memory_id.chars().take(12).collect();
        info!("✏️ Updating memory: {}...", id_preview);

        let result = self
            .client
            .update(&params.memory_id, &params.new_content, &params.user_id)
            .await
            .map_err(Self::convert_error)?;

        if result.updated {
            info!("✅ Memory updated");
        } else {
            warn!("⚠️ Memory update failed");
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
        info!("📊 Getting memory graph for user={}", params.user_id);

        let result = self
            .client
            .get_graph(
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
            "🎯 Concept search: '{}' type={:?}",
            query_preview, params.concept_type
        );

        let results = self
            .client
            .search_by_concept(
                &params.query,
                &params.user_id,
                params.concept_type.map(|c| c.as_str()),
                params.tags.as_deref(),
                params.mode.map(|m| m.as_str()),
                params.limit.map(|l| l as usize),
            )
            .await
            .map_err(Self::convert_error)?;

        info!("✅ Found {} memories", results.len());

        let json = Self::result_to_json(&results)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Reconstruct chains of reasoning around a topic — the 'why / what-follows' tool, and Helixir's signature capability. It finds seed memories then walks typed reasoning edges (BECAUSE/IMPLIES/SUPPORTS/CONTRADICTS) to assemble cause→effect chains with a human-readable reasoning_trail. Use chain_mode 'causal' for 'why is X so', 'forward' for 'what does X lead to', 'both'/'deep' for full context. Can return a LARGE payload on a dense graph — keep max_depth (default 5) and limit modest. Returns {query, chains:[{seed, nodes, reasoning_trail}], total_memories, deepest_chain}."
    )]
    async fn search_reasoning_chain(
        &self,
        Parameters(params): Parameters<SearchReasoningChainParams>,
    ) -> Result<CallToolResult, McpError> {
        let chain_mode = params.chain_mode.map(|c| c.as_str()).unwrap_or("both");

        let query_preview: String = params.query.chars().take(30).collect();
        info!(
            "🔗 Reasoning chain: '{}' mode={}",
            query_preview, chain_mode
        );

        let result = self
            .client
            .search_reasoning_chain(
                &params.query,
                &params.user_id,
                Some(chain_mode),
                params.max_depth.map(|d| d as usize),
                params.limit.map(|l| l as usize),
            )
            .await
            .map_err(Self::convert_error)?;

        info!("✅ Found {} chains", result.chains.len());

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
            "🌉 Connect: '{}' <-> '{}'",
            params.query_a.chars().take(30).collect::<String>(),
            params.query_b.chars().take(30).collect::<String>()
        );

        let result = self
            .client
            .connect_memories(
                &params.query_a,
                &params.query_b,
                &params.user_id,
                params.max_depth.map(|d| d as usize),
            )
            .await
            .map_err(Self::convert_error)?;

        info!("✅ Connection: found={} hops={}", result.found, result.hops);

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
        info!("🔍 Searching for incomplete thoughts");

        let limit = params.limit.unwrap_or(5) as usize;

        let results = self
            .client
            .tooling()
            .search_by_tag("incomplete_thought", limit)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

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
