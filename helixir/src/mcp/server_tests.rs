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
