//! `HelixirMcpServer` core: struct, constructor, error mapping, stdio
//! `run_server` entry point. Tool implementations live in [`super::tools`]
//! and the `ServerHandler` impl lives in [`super::handler`].

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use rmcp::{
    ErrorData as McpError, ServiceExt,
    handler::server::{router::prompt::PromptRouter, router::tool::ToolRouter},
    transport::stdio,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tracing::{info, warn};

use crate::core::config::HelixirConfig;
use crate::core::helixir_client::{HelixirClient, HelixirClientError};
use crate::toolkit::fast_think::{FastThinkLimits, FastThinkManager};
use crate::toolkit::tooling_manager::{ToolingManager, ingest_buffer};

struct IngestWorkerRuntime {
    tooling: Arc<arc_swap::ArcSwap<ToolingManager>>,
    task: parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl IngestWorkerRuntime {
    fn new(initial: Arc<ToolingManager>) -> Arc<Self> {
        let tooling = Arc::new(arc_swap::ArcSwap::from(initial));
        let task = ingest_buffer::buffer_enabled()
            .then(|| tokio::spawn(ingest_buffer::run_ingest_worker(Arc::clone(&tooling))));
        Self::with_task(tooling, task)
    }

    fn with_task(
        tooling: Arc<arc_swap::ArcSwap<ToolingManager>>,
        task: Option<tokio::task::JoinHandle<()>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            tooling,
            task: parking_lot::Mutex::new(task),
        })
    }

    fn update(&self, tooling: Arc<ToolingManager>) {
        self.tooling.store(tooling);
    }
}

impl Drop for IngestWorkerRuntime {
    fn drop(&mut self) {
        if let Some(task) = self.task.get_mut().take() {
            task.abort();
        }
    }
}

#[derive(Clone)]
pub struct HelixirMcpServer {
    /// #52: the client sits behind an ArcSwap so a SIGHUP can rebuild it
    /// from a freshly re-read config and swap it atomically — in-flight
    /// requests finish on the old client, new requests see the new one.
    pub(super) client: Arc<arc_swap::ArcSwap<HelixirClient>>,
    pub(super) fast_think: Arc<FastThinkManager>,
    ingest_worker: Arc<IngestWorkerRuntime>,
    pub(super) tool_router: ToolRouter<Self>,
    pub(super) prompt_router: PromptRouter<Self>,
}

impl HelixirMcpServer {
    pub fn new(client: HelixirClient) -> Self {
        let client_arc = Arc::new(client);
        let fast_think = Arc::new(FastThinkManager::new(
            client_arc.clone(),
            FastThinkLimits::from_config(&client_arc.config().fast_think),
        ));
        let ingest_worker = IngestWorkerRuntime::new(client_arc.tooling_arc());

        Self {
            client: Arc::new(arc_swap::ArcSwap::from(client_arc)),
            fast_think,
            ingest_worker,
            tool_router: Self::build_tool_router(),
            prompt_router: Self::build_prompt_router(),
        }
    }

    /// The current client. Load per call — never cache across an await if
    /// you want reloads to take effect promptly (holding one for a single
    /// request is exactly right).
    pub(super) fn client(&self) -> Arc<HelixirClient> {
        self.client.load_full()
    }

    /// #52: re-read the layered config (defaults -> helixir.toml -> env),
    /// build + initialize a NEW client, swap it in. The old client keeps
    /// serving whatever still holds it (in-flight requests, FastThink).
    /// On any failure the old client stays — reload is all-or-nothing.
    async fn reload(
        handle: &arc_swap::ArcSwap<HelixirClient>,
        fast_think: &FastThinkManager,
        ingest_worker: &IngestWorkerRuntime,
    ) -> anyhow::Result<()> {
        let config = HelixirConfig::from_env();
        let client = HelixirClient::new(config)?;
        client.initialize().await?;
        let client = Arc::new(client);
        Self::publish_generation(handle, fast_think, ingest_worker, client);
        info!("config reload: client, FastThink and ingest runtime generation swapped");
        Ok(())
    }

    fn publish_generation(
        handle: &arc_swap::ArcSwap<HelixirClient>,
        fast_think: &FastThinkManager,
        ingest_worker: &IngestWorkerRuntime,
        client: Arc<HelixirClient>,
    ) {
        let limits = FastThinkLimits::from_config(&client.config().fast_think);
        ingest_worker.update(client.tooling_arc());
        fast_think.update_runtime(Arc::clone(&client), limits);
        handle.store(client);
    }

    /// Spawn the SIGHUP listener that triggers [`Self::reload`].
    /// FastThink deliberately keeps its construction-time client: active
    /// scratchpad sessions must not lose their memory handle mid-thought.
    #[cfg(unix)]
    fn spawn_sighup_reload(
        handle: Arc<arc_swap::ArcSwap<HelixirClient>>,
        fast_think: Arc<FastThinkManager>,
        ingest_worker: Arc<IngestWorkerRuntime>,
    ) {
        tokio::spawn(async move {
            let Ok(mut hup) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
            else {
                warn!("config reload: cannot install SIGHUP handler");
                return;
            };
            while hup.recv().await.is_some() {
                info!("SIGHUP received: reloading config");
                if let Err(e) = Self::reload(&handle, &fast_think, &ingest_worker).await {
                    warn!("config reload FAILED — keeping the old client: {e}");
                }
            }
        });
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
    info!("Initializing Helixir MCP Server...");

    let config = HelixirConfig::from_env();
    let client = HelixirClient::new(config)?;
    client.initialize().await?;

    info!("Helixir MCP Server ready");
    info!(
        "   HelixDB: {}:{}",
        client.config().host,
        client.config().port
    );
    info!(
        "   LLM: {}/{}",
        client.config().llm_provider,
        client.config().llm_model
    );
    info!("   Instance: {}", client.config().instance);

    let server = HelixirMcpServer::new(client);
    let fast_think = Arc::clone(&server.fast_think);
    #[cfg(unix)]
    HelixirMcpServer::spawn_sighup_reload(
        Arc::clone(&server.client),
        Arc::clone(&server.fast_think),
        Arc::clone(&server.ingest_worker),
    );
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    // The client hung up (one-shot agents do this constantly) — reasoning
    // still in the scratchpad must not die with the process.
    let saved = fast_think.save_all_interrupted("mcp shutdown").await;
    if saved > 0 {
        info!("Auto-saved {saved} interrupted FastThink session(s) on shutdown");
    }

    Ok(())
}

/// The network gateway (#42): serve the SAME `HelixirMcpServer` over HTTP
/// (streamable-http) instead of stdio, so one process per host serves many
/// clients (local + remote) over the network — clients carry no HELIX_* env,
/// just the gateway URL. Coordination still happens in the shared DB; this is
/// the per-host serving layer on top of the rendezvous (#39). Full network
/// trust remains the default. Setting `gateway.auth_token` enables bearer
/// authentication; `require_auth` additionally fails closed when no token is
/// configured for this invocation.
pub async fn run_gateway(bind: &str) -> anyhow::Result<()> {
    run_gateway_with_options(bind, false).await
}

#[derive(Clone)]
struct GatewayAuthState {
    client: Arc<arc_swap::ArcSwap<HelixirClient>>,
    require_auth: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum GatewayAuthDecision {
    Allow,
    Unauthorized,
    MissingConfiguration,
}

fn gateway_auth_decision(
    configured_token: Option<&str>,
    authorization: Option<&str>,
    require_auth: bool,
) -> GatewayAuthDecision {
    let Some(expected) = configured_token.filter(|token| !token.is_empty()) else {
        return if require_auth {
            GatewayAuthDecision::MissingConfiguration
        } else {
            GatewayAuthDecision::Allow
        };
    };
    let Some((scheme, supplied)) = authorization.and_then(|value| value.split_once(' ')) else {
        return GatewayAuthDecision::Unauthorized;
    };
    let expected_digest = Sha256::digest(expected.as_bytes());
    let supplied_digest = Sha256::digest(supplied.as_bytes());
    if !scheme.eq_ignore_ascii_case("Bearer")
        || !bool::from(expected_digest.ct_eq(&supplied_digest))
    {
        return GatewayAuthDecision::Unauthorized;
    }
    GatewayAuthDecision::Allow
}

async fn gateway_auth(
    State(state): State<GatewayAuthState>,
    request: Request,
    next: Next,
) -> Response {
    let client = state.client.load();
    let authorization = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    match gateway_auth_decision(
        client.config().gateway.auth_token.as_deref(),
        authorization,
        state.require_auth,
    ) {
        GatewayAuthDecision::Allow => next.run(request).await,
        GatewayAuthDecision::Unauthorized => {
            let mut response = StatusCode::UNAUTHORIZED.into_response();
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
            response
        }
        GatewayAuthDecision::MissingConfiguration => (
            StatusCode::SERVICE_UNAVAILABLE,
            "gateway authentication is required but no token is configured",
        )
            .into_response(),
    }
}

/// Run the HTTP gateway with per-invocation security options.
pub async fn run_gateway_with_options(bind: &str, require_auth: bool) -> anyhow::Result<()> {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, tower::StreamableHttpService,
    };

    info!("Initializing Helixir MCP Gateway (#42)...");
    let config = HelixirConfig::from_env();
    let client = HelixirClient::new(config)?;
    client.initialize().await?;
    let auth_enabled = client
        .config()
        .gateway
        .auth_token
        .as_deref()
        .is_some_and(|token| !token.is_empty());
    info!(
        "Gateway ready — HelixDB {}:{}, instance {}, tier {}, auth {}",
        client.config().host,
        client.config().port,
        client.config().instance,
        client.config().mode.label(),
        if auth_enabled { "enabled" } else { "disabled" }
    );

    // One handler instance shared across sessions (the client is Arc'd); the
    // factory clones the template per session — cheap, no extra DB connections.
    let template = HelixirMcpServer::new(client);
    let auth_state = GatewayAuthState {
        client: Arc::clone(&template.client),
        require_auth,
    };
    #[cfg(unix)]
    HelixirMcpServer::spawn_sighup_reload(
        Arc::clone(&template.client),
        Arc::clone(&template.fast_think),
        Arc::clone(&template.ingest_worker),
    );
    let service = StreamableHttpService::new(
        move || Ok(template.clone()),
        Arc::new(LocalSessionManager::default()),
        Default::default(),
    );

    let app = axum::Router::new().nest_service("/mcp", service).layer(
        axum::middleware::from_fn_with_state(auth_state, gateway_auth),
    );
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!("Helixir gateway serving MCP at http://{bind}/mcp");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{future::pending, sync::Arc};

    use super::{
        GatewayAuthDecision, HelixirMcpServer, IngestWorkerRuntime, gateway_auth_decision,
        is_empty_user_graph_error,
    };
    use crate::{
        core::{config::HelixirConfig, helixir_client::HelixirClient},
        toolkit::fast_think::{FastThinkLimits, FastThinkManager},
    };

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

    #[test]
    fn gateway_auth_is_disabled_without_a_configured_token() {
        assert_eq!(
            gateway_auth_decision(None, None, false),
            GatewayAuthDecision::Allow
        );
    }

    #[test]
    fn gateway_auth_accepts_only_the_configured_bearer_token() {
        assert_eq!(
            gateway_auth_decision(Some("secret"), Some("Bearer secret"), false),
            GatewayAuthDecision::Allow
        );
        assert_eq!(
            gateway_auth_decision(Some("secret"), Some("Bearer wrong"), false),
            GatewayAuthDecision::Unauthorized
        );
        assert_eq!(
            gateway_auth_decision(Some("secret"), None, false),
            GatewayAuthDecision::Unauthorized
        );
    }

    #[test]
    fn gateway_can_fail_closed_when_auth_is_required() {
        assert_eq!(
            gateway_auth_decision(None, None, true),
            GatewayAuthDecision::MissingConfiguration
        );
    }

    #[tokio::test]
    async fn two_reload_generations_keep_one_ingest_worker() {
        let first = Arc::new(HelixirClient::new(HelixirConfig::default()).unwrap());
        let handle = arc_swap::ArcSwap::from(Arc::clone(&first));
        let fast_think = FastThinkManager::new(
            Arc::clone(&first),
            FastThinkLimits::from_config(&first.config().fast_think),
        );

        // Model the enabled-buffer branch with one inert task. Publishing a
        // new generation must only swap ToolingManager state; it must never
        // replace or multiply this process-owned worker.
        let tooling = Arc::new(arc_swap::ArcSwap::from(first.tooling_arc()));
        let worker = IngestWorkerRuntime::with_task(
            tooling,
            Some(tokio::spawn(async { pending::<()>().await })),
        );
        let worker_id = worker.task.lock().as_ref().unwrap().id();

        let second = Arc::new(HelixirClient::new(HelixirConfig::default()).unwrap());
        HelixirMcpServer::publish_generation(&handle, &fast_think, &worker, Arc::clone(&second));
        let third = Arc::new(HelixirClient::new(HelixirConfig::default()).unwrap());
        HelixirMcpServer::publish_generation(&handle, &fast_think, &worker, Arc::clone(&third));

        assert_eq!(worker.task.lock().as_ref().unwrap().id(), worker_id);
        assert!(Arc::ptr_eq(&handle.load_full(), &third));
        assert!(Arc::ptr_eq(
            &worker.tooling.load_full(),
            &third.tooling_arc()
        ));
    }
}
