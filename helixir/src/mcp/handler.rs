//! `#[prompt_router]` for [`super::HelixirMcpServer`] and the
//! `ServerHandler` trait implementation that wires tools / prompts /
//! resources together for `rmcp`.

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, handler::server::wrapper::Parameters,
    model::*, prompt, prompt_handler, prompt_router, service::RequestContext, tool_handler,
};
use serde_json::json;
use tokio::sync::broadcast;

use super::params::*;
use super::prompts;
use super::server::HelixirMcpServer;

impl HelixirMcpServer {
    /// Wrapper exposing the macro-generated `prompt_router()` across modules.
    /// Mirrors [`HelixirMcpServer::build_tool_router`] — `#[prompt_router]` emits
    /// a private associated function.
    pub(super) fn build_prompt_router() -> rmcp::handler::server::router::prompt::PromptRouter<Self>
    {
        Self::prompt_router()
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
        let topic_filter = args
            .topic
            .map(|t| format!(" about {}", t))
            .unwrap_or_default();

        let messages = vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!(
                "Analyze memories for user_id={}{}.

Use search_memory tool to find relevant memories.
Provide a summary with:
1. Key patterns and themes
2. Important facts and preferences  
3. Connections between memories
4. Timeline of events",
                args.user_id, topic_filter
            ),
        )];

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

        let messages = vec![PromptMessage::new_text(
            PromptMessageRole::Assistant,
            guide.to_string(),
        )];

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
                name: env!("CARGO_PKG_NAME").into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            instructions: Some(prompts::get_server_instructions()),
        }
    }

    /// Capture the peer once the client is ready and, when the ingest buffer
    /// is on, forward worker completion events as best-effort logging
    /// notifications (#25 phase 2). Authoritative delivery is still the
    /// opportunistic outbox — this just pushes a heads-up sooner for clients
    /// that surface notifications.
    async fn on_initialized(&self, context: rmcp::service::NotificationContext<RoleServer>) {
        if !crate::toolkit::tooling_manager::ingest_buffer::buffer_enabled() {
            return;
        }
        let peer = context.peer.clone();
        let mut rx = crate::toolkit::tooling_manager::ingest_buffer::subscribe_notify();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let _ = peer
                            .notify_logging_message(LoggingMessageNotificationParam {
                                level: LoggingLevel::Info,
                                logger: Some("helixir.ingest".to_string()),
                                data: json!({
                                    "kind": event.kind,
                                    "user_id": event.user_id,
                                    "message": event.summary,
                                }),
                            })
                            .await;
                    }
                    // Lagged (slow consumer) — keep going; Closed — stop.
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![
                RawResource::new("config://helixir", "helixir-config".to_string()).no_annotation(),
                RawResource::new("status://helixdb", "helixdb-status".to_string()).no_annotation(),
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
                    "version": env!("CARGO_PKG_VERSION"),
                    "helixdb": {
                        "host": config.host,
                        "port": config.port,
                        "instance": config.instance,
                    },
                    "llm": {
                        "provider": config.llm_provider,
                        "model": config.llm_model,
                    },
                    "retrieval_profile": crate::core::RetrievalProfile::cached().tag(),
                    "capabilities": {
                        "memory_management": true,
                        "hybrid_search_bm25_rrf": true,
                        "graph_traversal_batched": true,
                        "ppr_ranking": true,
                        "result_provenance": true,
                        "llm_free_read_path": true,
                        "llm_extraction": true,
                        "entity_linking": true,
                        "ontology_mapping": true,
                        "reasoning_chains": true,
                        "connect_memories": true,
                        "memory_charter_escalations": true,
                        "hive_stances": true,
                        "self_seed": true,
                    },
                    "tools": [
                        "add_memory",
                        "search_memory",
                        "search_by_concept",
                        "search_reasoning_chain",
                        "connect_memories",
                        "get_memory_graph",
                        "update_memory",
                        "list_memories",
                        "think_start",
                        "think_add",
                        "think_recall",
                        "think_conclude",
                        "think_commit",
                        "think_discard",
                        "think_status",
                        "search_incomplete_thoughts",
                    ],
                    "notes": {
                        "needs_clarification": "add_memory responses may carry charter escalations the agent should surface to the user",
                        "charter": "memory-charter.md — conflicts the engine never resolves silently",
                    },
                }))
                .unwrap_or_default();

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
                }))
                .unwrap_or_default();

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
