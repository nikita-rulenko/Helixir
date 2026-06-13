//! `HelixirMcpServer` core: struct, constructor, error mapping, stdio
//! `run_server` entry point. Tool implementations live in [`super::tools`]
//! and the `ServerHandler` impl lives in [`super::handler`].

use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, ServiceExt,
    handler::server::{router::prompt::PromptRouter, router::tool::ToolRouter},
    transport::stdio,
};
use serde::Serialize;
use tracing::info;

use crate::core::config::HelixirConfig;
use crate::core::helixir_client::{HelixirClient, HelixirClientError};
use crate::toolkit::fast_think::{FastThinkLimits, FastThinkManager};

#[derive(Clone)]
pub struct HelixirMcpServer {
    pub(super) client: Arc<HelixirClient>,
    pub(super) fast_think: Arc<FastThinkManager>,
    pub(super) tool_router: ToolRouter<Self>,
    pub(super) prompt_router: PromptRouter<Self>,
}

impl HelixirMcpServer {
    pub fn new(client: HelixirClient) -> Self {
        let client_arc = Arc::new(client);
        let fast_think = Arc::new(FastThinkManager::new(
            client_arc.clone(),
            FastThinkLimits::mcp(),
        ));

        Self {
            client: client_arc,
            fast_think,
            tool_router: Self::build_tool_router(),
            prompt_router: Self::build_prompt_router(),
        }
    }

    pub(super) fn convert_error(err: HelixirClientError) -> McpError {
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

    pub(super) fn result_to_json<T: Serialize>(result: T) -> Result<String, McpError> {
        serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    /// Like `result_to_json` but returns a `Value` so callers can splice in
    /// extra fields (e.g. opportunistic `pending_outcomes`) before serializing.
    pub(super) fn result_to_value<T: Serialize>(result: T) -> Result<serde_json::Value, McpError> {
        serde_json::to_value(&result).map_err(|e| McpError::internal_error(e.to_string(), None))
    }
}

/// Returns true when HelixDB's error message indicates that a user-scoped
/// traversal failed because the User node has no outgoing edges (or doesn't
/// exist yet). Callers should treat this as "empty result", not as a hard
/// failure. See issue #19.
///
/// HelixDB raises (with `code: GRAPH_ERROR`) the literal string
/// `"Graph error: No value found"` when `N<User>::FIRST` returns nothing or
/// when a subsequent traversal step has no items to walk. Be specific:
/// match on the `"no value found"` token so that unrelated `GRAPH_ERROR`
/// signals (syntax errors, missing indexes, etc.) still surface as hard
/// failures.
pub(super) fn is_empty_user_graph_error(msg: &str) -> bool {
    msg.to_lowercase().contains("no value found")
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

#[cfg(test)]
mod tests {
    use super::is_empty_user_graph_error;

    #[test]
    fn empty_user_graph_error_matches_helixdb_payload() {
        let msg = r#"Query failed: Got Error from server: {"error":"Graph error: No value found","code":"GRAPH_ERROR"}"#;
        assert!(is_empty_user_graph_error(msg));
    }

    #[test]
    fn empty_user_graph_error_is_case_insensitive() {
        assert!(is_empty_user_graph_error(
            "Graph error: NO VALUE FOUND somewhere"
        ));
    }

    #[test]
    fn empty_user_graph_error_does_not_match_unrelated_graph_errors() {
        // Other GRAPH_ERROR causes (schema mismatch, missing index) must NOT
        // be silently swallowed.
        let msg = r#"{"error":"Graph error: type mismatch on field","code":"GRAPH_ERROR"}"#;
        assert!(!is_empty_user_graph_error(msg));
    }
}
