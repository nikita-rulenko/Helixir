use super::*;

#[test]
fn default_bind_is_loopback() {
    let config = ControlPlaneConfig::default();
    assert!(config.bind.ip().is_loopback());
    assert!(!config.containerized);
}

#[test]
fn wildcard_bind_is_reserved_for_container_mode() {
    let mut config = ControlPlaneConfig {
        bind: std::net::SocketAddr::from(([0, 0, 0, 0], 6971)),
        ..ControlPlaneConfig::default()
    };
    assert!(validate_bind(&config).is_err());
    config.containerized = true;
    assert!(validate_bind(&config).is_ok());
}

#[test]
fn container_host_operations_fail_closed() {
    let state = AppState {
        session_token: Arc::from("test-token"),
        actor_id: Arc::from("codex"),
        db: Arc::new(crate::db::HelixClient::new("127.0.0.1", 1).unwrap()),
        admin_required: true,
        containerized: true,
        supervisor: None,
        category_graph: CategoryGraphCache::default(),
    };
    let Err((status, problem)) = require_host_operations(&state) else {
        panic!("container mode without a supervisor must fail closed");
    };
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(problem.0.code, "host_supervisor_unavailable");
    let native = AppState {
        containerized: false,
        ..state
    };
    assert!(require_host_operations(&native).is_ok());
}

#[test]
fn missing_supervisor_operation_remains_a_not_found_response() {
    let (status, problem) = supervisor_error(anyhow::anyhow!(
        "host supervisor returned 404 Not Found: operation was not found"
    ));
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(problem.0.code, "operation_not_found");
}
