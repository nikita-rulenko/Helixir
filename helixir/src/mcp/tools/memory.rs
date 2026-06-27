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
        description = "Add memory with LLM-powered extraction. Extracts atomic facts (max 15 per call), generates embeddings, creates graph relations. For large texts (>15 facts expected), split into smaller chunks before calling. Returns: {memories_added, memory_ids, deduped, entities, relations, chunks_created, stats}. The 'deduped' array holds existing memory_ids this input was already-known-and-linked-to (not newly stored) — so memories_added=0 with a non-empty deduped means 'already saved', not a failure. IMPORTANT: if the response contains a needs_clarification array, the memory charter blocked silent resolution of a conflict — read each entry and ask the user its suggested_question (or apply a standing rule), then add the answer as a new memory."
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
        description = "Smart memory search with automatic strategy selection. Modes: 'recent' (4h, fast), 'contextual' (30d, balanced), 'deep' (90d), 'full' (all). Scope: 'personal' (this user only), 'collective' (all users, ranked by consensus), 'all' (combined with controversy annotations). Returns: [{memory_id, content, score, metadata}]"
    )]
    async fn search_memory(
        &self,
        Parameters(params): Parameters<SearchMemoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let mode = params
            .mode
            .unwrap_or_else(|| self.client.config().default_search_mode.clone());
        let limit = params.limit.map(|l| l as usize);
        // Default scope is intentionally personal (GH #40): collective memory
        // stays hidden unless explicitly requested, so weak models aren't
        // flooded with other users' facts. Not a config knob — a safety default.
        let requested_scope = params.scope.unwrap_or_else(|| "personal".to_string());
        // Solo mode answers only from the user's own memory — a collective/all
        // request is downgraded to personal rather than leaking other users'.
        let scope = if self.client.config().mode.collective_enabled() {
            requested_scope
        } else {
            "personal".to_string()
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
                Some(&scope),
            )
            .await
            .map_err(Self::convert_error)?;

        info!("✅ Found {} memories", results.len());

        let json = Self::result_to_json(&results)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "List all memories for a user without semantic search. Use for exhaustive queries, full-scan, counting, or when you need to see everything in the memory store. Returns: [{memory_id, content, memory_type, created_at, importance, certainty}]"
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

        if let Some(ref mem_type) = params.memory_type {
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
        description = "Update memory content (regenerates embedding & relations). Returns: {updated: bool, memory_id}"
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

    #[tool(description = "Get memory graph visualization. Returns: {nodes: [...], edges: [...]}")]
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
        description = "Search memories by ontology concepts. Concept types: 'skill', 'preference', 'goal', 'fact', 'opinion', 'experience', 'achievement', 'action'. Returns: [{memory_id, content, concept_score}]"
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
                params.concept_type.as_deref(),
                params.tags.as_deref(),
                params.mode.as_deref(),
                params.limit.map(|l| l as usize),
            )
            .await
            .map_err(Self::convert_error)?;

        info!("✅ Found {} memories", results.len());

        let json = Self::result_to_json(&results)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Search with logical reasoning chains (IMPLIES/BECAUSE/CONTRADICTS). Chain modes: 'causal' (why?), 'forward' (effects), 'both', 'deep'. Returns: {chains: [...], deepest_chain}"
    )]
    async fn search_reasoning_chain(
        &self,
        Parameters(params): Parameters<SearchReasoningChainParams>,
    ) -> Result<CallToolResult, McpError> {
        let chain_mode = params.chain_mode.unwrap_or_else(|| "both".to_string());

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
                Some(&chain_mode),
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
