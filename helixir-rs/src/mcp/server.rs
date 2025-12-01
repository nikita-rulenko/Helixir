use rmcp::{
    handler::server::{
        router::tool::ToolRouter,
        router::prompt::PromptRouter,
        wrapper::Parameters,
    },
    model::*,
    tool, tool_handler, tool_router,
    prompt, prompt_handler, prompt_router,
    transport::stdio,
    service::RequestContext,
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use tracing::{info, warn};

use crate::core::config::HelixirConfig;
use crate::core::helixir_client::{HelixirClient, HelixirClientError};
use crate::toolkit::fast_think::{FastThinkManager, FastThinkLimits, FastThinkError, ThoughtType};

use super::params::*;
use super::prompts;

#[derive(Clone)]
pub struct HelixirMcpServer {
    client: Arc<HelixirClient>,
    fast_think: Arc<FastThinkManager>,
    tool_router: ToolRouter<Self>,
    prompt_router: PromptRouter<Self>,
}

impl HelixirMcpServer {
    pub fn new(client: HelixirClient) -> Self {
        let client_arc = Arc::new(client);
        let fast_think = Arc::new(FastThinkManager::new(
            client_arc.clone(),
            FastThinkLimits::default(),
        ));
        
        Self {
            client: client_arc,
            fast_think,
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }

    fn convert_error(err: HelixirClientError) -> McpError {
        match err {
            HelixirClientError::Config(msg) => McpError::invalid_params(msg, None),
            HelixirClientError::Database(msg) => McpError::internal_error(msg, None),
            HelixirClientError::Llm(msg) => McpError::internal_error(msg, None),
            HelixirClientError::Embedding(msg) => McpError::internal_error(msg, None),
            HelixirClientError::Tooling(msg) => McpError::internal_error(msg, None),
            HelixirClientError::NotInitialized => {
                McpError::internal_error("Client not initialized", None)
            }
            HelixirClientError::Operation(msg) => McpError::internal_error(msg, None),
        }
    }

    fn result_to_json<T: Serialize>(result: T) -> Result<String, McpError> {
        serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))
    }
}

#[tool_router]
impl HelixirMcpServer {
    #[tool(description = "Add memory with LLM-powered extraction. Extracts atomic facts, generates embeddings, creates graph relations. Returns: {memories_added, entities, relations, memory_ids, chunks_created}")]
    async fn add_memory(
        &self,
        Parameters(params): Parameters<AddMemoryParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("🧠 Adding memory for user={}", params.user_id);

        let result = self.client
            .add(&params.message, &params.user_id, params.agent_id.as_deref(), None)
            .await
            .map_err(Self::convert_error)?;

        info!(
            "✅ Added {} memories ({} chunks)",
            result.memories_added,
            result.chunks_created
        );

        let json = Self::result_to_json(&result)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Smart memory search with automatic strategy selection. Modes: 'recent' (4h, fast), 'contextual' (30d, balanced), 'deep' (90d), 'full' (all). Returns: [{memory_id, content, score, metadata}]")]
    async fn search_memory(
        &self,
        Parameters(params): Parameters<SearchMemoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let mode = params.mode.unwrap_or_else(|| "recent".to_string());
        let limit = params.limit.map(|l| l as usize);

        let query_preview: String = params.query.chars().take(50).collect();
        info!(
            "🔍 Searching: '{}' [mode={}, limit={:?}]",
            query_preview, mode, limit
        );

        let results = self.client
            .search(
                &params.query,
                &params.user_id,
                limit,
                Some(&mode),
                params.temporal_days,
                params.graph_depth.map(|d| d as usize),
            )
            .await
            .map_err(Self::convert_error)?;

        info!("✅ Found {} memories", results.len());

        let json = Self::result_to_json(&results)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Update memory content (regenerates embedding & relations). Returns: {updated: bool, memory_id}")]
    async fn update_memory(
        &self,
        Parameters(params): Parameters<UpdateMemoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let id_preview: String = params.memory_id.chars().take(12).collect();
        info!("✏️ Updating memory: {}...", id_preview);

        let result = self.client
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

        let result = self.client
            .get_graph(&params.user_id, params.memory_id.as_deref(), params.depth.map(|d| d as usize))
            .await
            .map_err(Self::convert_error)?;

        let json = Self::result_to_json(&result)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Search memories by ontology concepts. Concept types: 'skill', 'preference', 'goal', 'fact', 'opinion', 'experience', 'achievement'. Returns: [{memory_id, content, concept_score}]")]
    async fn search_by_concept(
        &self,
        Parameters(params): Parameters<SearchByConceptParams>,
    ) -> Result<CallToolResult, McpError> {
        let query_preview: String = params.query.chars().take(30).collect();
        info!(
            "🎯 Concept search: '{}' type={:?}",
            query_preview, params.concept_type
        );

        let results = self.client
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

    #[tool(description = "Search with logical reasoning chains (IMPLIES/BECAUSE/CONTRADICTS). Chain modes: 'causal' (why?), 'forward' (effects), 'both', 'deep'. Returns: {chains: [...], deepest_chain}")]
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

        let result = self.client
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

    #[tool(description = "Start a FastThink session for reasoning. Creates an isolated working memory graph. Returns: {session_id, root_thought_idx}")]
    async fn think_start(
        &self,
        Parameters(params): Parameters<StartThinkingParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("🧠 Starting thinking session: {}", params.session_id);

        let result = self.fast_think
            .start_thinking(&params.session_id, &params.initial_thought)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let json = Self::result_to_json(&json!({
            "session_id": params.session_id,
            "root_thought_idx": result.index(),
            "status": "thinking"
        }))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Add a thought to an active FastThink session. Returns: {thought_idx, thought_count, depth}")]
    async fn think_add(
        &self,
        Parameters(params): Parameters<AddThoughtParams>,
    ) -> Result<CallToolResult, McpError> {
        let thought_type = match params.thought_type.as_deref() {
            Some("hypothesis") => ThoughtType::Hypothesis,
            Some("observation") => ThoughtType::Observation,
            Some("question") => ThoughtType::Question,
            _ => ThoughtType::Reasoning,
        };

        let parent = params.parent_idx.map(|idx| petgraph::stable_graph::NodeIndex::new(idx as usize));

        let result = self.fast_think.add_thought(
            &params.session_id,
            &params.content,
            thought_type,
            parent,
            None,
        );

        match result {
            Ok(node) => {
                let status = self.fast_think
                    .get_session_status(&params.session_id)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;

                let json = Self::result_to_json(&json!({
                    "thought_idx": node.index(),
                    "thought_count": status.thought_count,
                    "depth": status.current_depth
                }))?;
                Ok(CallToolResult::success(vec![Content::text(json)]))
            }
            Err(FastThinkError::Timeout) => {
                warn!("⏰ FastThink timeout - committing partial results");
                let commit_result = self.fast_think
                    .commit_partial(&params.session_id, "claude", "timeout")
                    .await;

                match commit_result {
                    Ok(cr) => {
                        let json = Self::result_to_json(&json!({
                            "status": "timeout_committed",
                            "memory_id": cr.memory_id,
                            "thoughts_saved": cr.thoughts_processed,
                            "message": "⚠️ Thinking timed out. Partial thoughts saved to memory for future research."
                        }))?;
                        Ok(CallToolResult::success(vec![Content::text(json)]))
                    }
                    Err(e) => Err(McpError::internal_error(format!("Timeout and commit failed: {}", e), None))
                }
            }
            Err(e) => Err(McpError::internal_error(e.to_string(), None))
        }
    }

    #[tool(description = "Recall facts from main memory into FastThink session. READ-ONLY access to main memory. Returns: {recalled_count, thought_indices}")]
    async fn think_recall(
        &self,
        Parameters(params): Parameters<ThinkRecallParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("💭 Recalling from main memory: '{}'", params.query);

        let parent = petgraph::stable_graph::NodeIndex::new(params.parent_idx as usize);

        let results = self.fast_think
            .recall(&params.session_id, &params.query, parent)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let indices: Vec<usize> = results.iter().map(|n| n.index()).collect();

        info!("✅ Recalled {} facts", results.len());

        let json = Self::result_to_json(&json!({
            "recalled_count": results.len(),
            "thought_indices": indices
        }))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Mark a conclusion in FastThink session. Required before commit. Returns: {conclusion_idx, status}")]
    async fn think_conclude(
        &self,
        Parameters(params): Parameters<ThinkConcludeParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("✨ Concluding thinking session: {}", params.session_id);

        let supporting: Vec<petgraph::stable_graph::NodeIndex> = params
            .supporting_idx
            .unwrap_or_default()
            .iter()
            .map(|&idx| petgraph::stable_graph::NodeIndex::new(idx as usize))
            .collect();

        let result = self.fast_think
            .conclude(&params.session_id, &params.conclusion, &supporting)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let json = Self::result_to_json(&json!({
            "conclusion_idx": result.index(),
            "status": "decided"
        }))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Commit FastThink session to main memory. Writes conclusion with supporting evidence. Returns: {memory_id, thoughts_processed, elapsed_ms}")]
    async fn think_commit(
        &self,
        Parameters(params): Parameters<ThinkCommitParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("📝 Committing thinking session: {}", params.session_id);

        let result = self.fast_think
            .commit(&params.session_id, &params.user_id)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        info!(
            "✅ Committed: {} thoughts → memory {}",
            result.thoughts_processed, result.memory_id
        );

        let json = Self::result_to_json(&json!({
            "memory_id": result.memory_id,
            "thoughts_processed": result.thoughts_processed,
            "entities_extracted": result.entities_extracted,
            "concepts_mapped": result.concepts_mapped,
            "elapsed_ms": result.elapsed.as_millis()
        }))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Discard FastThink session without saving. Clears working memory. Returns: {discarded_thoughts, elapsed_ms}")]
    async fn think_discard(
        &self,
        Parameters(params): Parameters<ThinkDiscardParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("🗑️ Discarding thinking session: {}", params.session_id);

        let result = self.fast_think
            .discard(&params.session_id)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let json = Self::result_to_json(&json!({
            "discarded_thoughts": result.thoughts_discarded,
            "elapsed_ms": result.elapsed.as_millis()
        }))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Get status of a FastThink session. Returns: {status, thought_count, depth, has_conclusion, elapsed_ms}")]
    async fn think_status(
        &self,
        Parameters(params): Parameters<ThinkStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        let status = self.fast_think
            .get_session_status(&params.session_id)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let json = Self::result_to_json(&json!({
            "session_id": status.id,
            "status": status.status.to_string(),
            "thought_count": status.thought_count,
            "entity_count": status.entity_count,
            "concept_count": status.concept_count,
            "current_depth": status.current_depth,
            "has_conclusion": status.has_conclusion,
            "elapsed_ms": status.elapsed.as_millis()
        }))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Find incomplete thoughts from previous sessions that timed out. Use at session start to continue unfinished research. Returns: [{memory_id, content, created_at}]")]
    async fn search_incomplete_thoughts(
        &self,
        Parameters(params): Parameters<SearchIncompleteThoughtsParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("🔍 Searching for incomplete thoughts");

        let limit = params.limit.unwrap_or(5) as usize;

        let results = self.client.tooling()
            .search_by_tag("incomplete_thought", limit)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        if results.is_empty() {
            let json = Self::result_to_json(&json!({
                "found": 0,
                "message": "No incomplete thoughts found"
            }))?;
            return Ok(CallToolResult::success(vec![Content::text(json)]));
        }

        let json = Self::result_to_json(&json!({
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

#[prompt_router]
impl HelixirMcpServer {
    #[prompt(
        name = "memory_summary",
        description = "Generate prompt to summarize user's memories on a topic"
    )]
    async fn memory_summary(
        &self,
        Parameters(args): Parameters<MemorySummaryArgs>,
    ) -> Result<GetPromptResult, McpError> {
        let topic_filter = args.topic
            .map(|t| format!(" about {}", t))
            .unwrap_or_default();

        let messages = vec![
            PromptMessage::new_text(
                PromptMessageRole::User,
                format!(
                    "Analyze memories for user_id={}{}.

Use search_memory tool to find relevant memories.
Provide a summary with:
1. Key patterns and themes
2. Important facts and preferences  
3. Connections between memories
4. Timeline of events",
                    args.user_id,
                    topic_filter
                ),
            ),
        ];

        Ok(GetPromptResult {
            description: Some(format!("Memory summary for {}", args.user_id)),
            messages,
        })
    }

    #[prompt(
        name = "tool_selection_guide",
        description = "Cognitive protocol for AI agents with persistent memory"
    )]
    async fn tool_selection_guide(&self) -> Result<GetPromptResult, McpError> {
        let guide = prompts::get_cognitive_protocol();

        let messages = vec![
            PromptMessage::new_text(PromptMessageRole::Assistant, guide.to_string()),
        ];

        Ok(GetPromptResult {
            description: Some("Tool selection guide for Helixir memory operations".to_string()),
            messages,
        })
    }
}

#[tool_handler]
#[prompt_handler]
impl ServerHandler for HelixirMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
            server_info: Implementation {
                name: "helixir".into(),
                version: "2.0.0".into(),
                ..Default::default()
            },
            instructions: Some(prompts::get_server_instructions()),
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![
                RawResource::new("config://helixir", "helixir-config".to_string())
                    .no_annotation(),
                RawResource::new("status://helixdb", "helixdb-status".to_string())
                    .no_annotation(),
            ],
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        ReadResourceRequestParam { uri }: ReadResourceRequestParam,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        match uri.as_str() {
            "config://helixir" => {
                let config = self.client.config();
                
                let content = serde_json::to_string_pretty(&json!({
                    "version": "2.0.0",
                    "helixdb": {
                        "host": config.host,
                        "port": config.port,
                        "instance": config.instance,
                    },
                    "llm": {
                        "provider": config.llm_provider,
                        "model": config.llm_model,
                    },
                    "capabilities": {
                        "memory_management": true,
                        "vector_search": true,
                        "graph_traversal": true,
                        "llm_extraction": true,
                        "entity_linking": true,
                        "ontology_mapping": true,
                        "onto_search": true,
                        "reasoning_chains": true,
                    },
                    "tools": [
                        "add_memory",
                        "search_memory",
                        "search_by_concept",
                        "search_reasoning_chain",
                        "get_memory_graph",
                        "update_memory",
                        "think_start",
                        "think_add",
                        "think_recall",
                        "think_conclude",
                        "think_commit",
                        "think_discard",
                        "think_status",
                        "search_incomplete_thoughts",
                    ],
                })).unwrap_or_default();

                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(content, uri)],
                })
            }
            "status://helixdb" => {
                let config = self.client.config();
                
                let content = serde_json::to_string_pretty(&json!({
                    "status": "connected",
                    "host": config.host,
                    "port": config.port,
                    "instance": config.instance,
                })).unwrap_or_default();

                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(content, uri)],
                })
            }
            _ => Err(McpError::resource_not_found(
                format!("Unknown resource: {}", uri),
                Some(json!({ "uri": uri })),
            )),
        }
    }
}

pub async fn run_server() -> anyhow::Result<()> {
    info!("🚀 Initializing Helixir MCP Server...");

    let config = HelixirConfig::from_env();
    let client = HelixirClient::new(config)?;
    client.initialize().await?;

    info!("✅ Helixir MCP Server ready");
    info!(
        "   📍 HelixDB: {}:{}",
        client.config().host,
        client.config().port
    );
    info!(
        "   🤖 LLM: {}/{}",
        client.config().llm_provider,
        client.config().llm_model
    );
    info!("   📊 Instance: {}", client.config().instance);

    let server = HelixirMcpServer::new(client);
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
