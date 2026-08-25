//! Axum data plane and disabled-by-default loopback admin plane.

use crate::config::Config;
use crate::fixture::{FixtureError, build_response};
use crate::metrics::Metrics;
use crate::profile::{Profile, Scenario};
use crate::registry::{ParamSpec, QuerySpec, find_query, manifest};
use crate::response::{Completion, finish};
use crate::state::StateStore;
use crate::trace::TraceSink;
use anyhow::{Context, Result};
use axum::Router;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::sync::{Mutex, watch};

/// Shared bounded emulator state used by Axum routers.
#[derive(Clone)]
pub struct AppState {
    pub(crate) profile: Profile,
    scenario: Scenario,
    seed: u64,
    max_response_bytes: usize,
    store: Arc<Mutex<StateStore>>,
    pub(crate) metrics: Arc<Metrics>,
    pub(crate) trace: TraceSink,
    request_sequence: Arc<AtomicU64>,
}

impl AppState {
    /// Validate configuration and create a coherently seeded state.
    ///
    /// # Errors
    ///
    /// Returns invalid configuration and trace initialization failures.
    pub fn new(config: &Config) -> Result<Self> {
        config.validate()?;
        let mut store = StateStore::new(config.max_records);
        store.seed(config.scenario);
        Ok(Self {
            profile: config.profile,
            scenario: config.scenario,
            seed: config.seed,
            max_response_bytes: config.max_response_bytes,
            store: Arc::new(Mutex::new(store)),
            metrics: Arc::new(Metrics::default()),
            trace: TraceSink::open(config.trace_path.as_deref())?,
            request_sequence: Arc::new(AtomicU64::new(1)),
        })
    }

    /// Active latency/density profile.
    #[must_use]
    pub fn profile(&self) -> Profile {
        self.profile
    }
}

/// Build the `HelixDB`-compatible data-plane router.
pub fn data_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health).post(health))
        .route("/{query_name}", post(query))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(state)
}

/// Build the loopback-only diagnostics router.
pub fn admin_router(state: AppState) -> Router {
    Router::new()
        .route("/metrics", get(admin_metrics))
        .route("/registry", get(admin_registry))
        .route("/control/reset", post(admin_reset))
        .with_state(state)
}

/// Run the data plane and optional admin plane until shutdown.
///
/// # Errors
///
/// Returns listener, server, signal, and task-join failures.
pub async fn run(config: Config) -> Result<()> {
    config.validate()?;
    let state = AppState::new(&config)?;
    let data_listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .with_context(|| format!("bind data listener {}", config.listen))?;
    let admin_listener = match config.admin_listen {
        Some(address) => Some((
            address,
            tokio::net::TcpListener::bind(address)
                .await
                .with_context(|| format!("bind admin listener {address}"))?,
        )),
        None => None,
    };

    tracing::info!(
        listen = %config.listen,
        profile = state.profile().as_str(),
        scenario = state.scenario.as_str(),
        query_count = crate::registry::QUERY_SPECS.len(),
        "helixdb-mock data plane ready"
    );
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let admin_task = admin_listener.map(|(address, listener)| {
        tracing::info!(listen = %address, "helixdb-mock loopback admin plane ready");
        let shutdown = shutdown_rx.clone();
        let router = admin_router(state.clone());
        tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(wait_for_shutdown(shutdown))
                .await
        })
    });

    let data_result = axum::serve(data_listener, data_router(state))
        .with_graceful_shutdown(async move {
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::error!(%error, "failed to install shutdown signal");
            }
            let _ = shutdown_tx.send(true);
        })
        .await;
    if let Some(task) = admin_task {
        task.await.context("join admin server")??;
    }
    data_result.context("data server")
}

async fn wait_for_shutdown(mut shutdown: watch::Receiver<bool>) {
    while !*shutdown.borrow() && shutdown.changed().await.is_ok() {}
}

async fn health() -> impl IntoResponse {
    axum::Json(json!({
        "ok": true,
        "service": "helixdb-mock",
        "query_count": crate::registry::QUERY_SPECS.len(),
        "schema_sha256": crate::registry::SCHEMA_SHA256,
    }))
}

// Keeping validation, deterministic delay, dependency enforcement and response
// completion in one private handler makes every exit observable in the same way.
#[allow(clippy::too_many_lines)]
async fn query(
    State(state): State<AppState>,
    Path(query_name): Path<String>,
    axum::Json(params): axum::Json<Value>,
) -> Response<Body> {
    let started = Instant::now();
    let request_id = state.request_sequence.fetch_add(1, Ordering::Relaxed);
    let request_bytes = serde_json::to_vec(&params).map_or(0, |body| body.len());
    let (body_hash, request_hash) = hash_request(&params);
    let parameter_names = parameter_names(&params);
    let state_before = state.store.lock().await.len();

    let Some(spec) = find_query(&query_name) else {
        let body = json!({"error":"Query not found","code":"QUERY_NOT_FOUND"});
        return finish_response(
            &state,
            request_id,
            &query_name,
            StatusCode::NOT_FOUND,
            body,
            started,
            request_bytes,
            &body_hash,
            &parameter_names,
            state_before,
            state_before,
        )
        .await;
    };
    if let Err(message) = validate_params(spec, &params) {
        let body = json!({"error":message,"code":"INVALID_PARAMS"});
        return finish_response(
            &state,
            request_id,
            &query_name,
            StatusCode::BAD_REQUEST,
            body,
            started,
            request_bytes,
            &body_hash,
            &parameter_names,
            state_before,
            state_before,
        )
        .await;
    }

    tokio::time::sleep(state.profile.latency(spec, state.seed, request_hash)).await;
    if state.scenario.inject_error() {
        return finish_response(
            &state,
            request_id,
            &query_name,
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error":"deterministic error scenario","code":"SCENARIO_ERROR"}),
            started,
            request_bytes,
            &body_hash,
            &parameter_names,
            state_before,
            state_before,
        )
        .await;
    }
    let (result, state_after) = {
        let mut store = state.store.lock().await;
        let result = if required_lookups_satisfied(spec, &params, &store) {
            build_response(
                spec,
                &params,
                &mut store,
                state.profile,
                state.scenario,
                state.seed,
                state.max_response_bytes,
            )
        } else {
            Err(FixtureError::MissingRequired(query_name.clone()))
        };
        (result, store.len())
    };
    let (status, body) = match result {
        Ok(body) => (StatusCode::OK, body),
        Err(FixtureError::MissingRequired(_)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({"error":"Graph error: No value found","code":"GRAPH_ERROR"}),
        ),
        Err(error) => {
            tracing::error!(query = %query_name, %error, "fixture generation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error":"mock response generation failed","code":"MOCK_ERROR"}),
            )
        }
    };
    finish_response(
        &state,
        request_id,
        &query_name,
        status,
        body,
        started,
        request_bytes,
        &body_hash,
        &parameter_names,
        state_before,
        state_after,
    )
    .await
}

fn required_lookups_satisfied(spec: &QuerySpec, params: &Value, store: &StateStore) -> bool {
    spec.required_lookups.iter().all(|lookup| {
        let literal = lookup.literal.map(|value| Value::String(value.to_owned()));
        lookup
            .parameter
            .and_then(|parameter| params.get(parameter))
            .or(literal.as_ref())
            .is_some_and(|value| store.contains_lookup(lookup.collection, lookup.property, value))
    })
}

// This private adapter keeps four early-exit sites uniform; `Completion`
// immediately packages the transport fields before metrics/trace handling.
#[allow(clippy::too_many_arguments)]
async fn finish_response(
    state: &AppState,
    request_id: u64,
    query: &str,
    status: StatusCode,
    body: Value,
    started: Instant,
    request_bytes: usize,
    body_hash: &str,
    parameter_names: &[String],
    state_records_before: usize,
    state_records_after: usize,
) -> Response<Body> {
    finish(
        state,
        Completion {
            request_id,
            query,
            status,
            body,
            started,
            request_bytes,
            request_hash: body_hash,
            parameter_names,
            state_records_before,
            state_records_after,
        },
    )
    .await
}

fn validate_params(query: &QuerySpec, params: &Value) -> Result<(), String> {
    let values = params
        .as_object()
        .ok_or_else(|| "request body must be a JSON object".to_owned())?;
    for spec in query.params {
        let value = values
            .get(spec.name)
            .ok_or_else(|| format!("missing parameter {}", spec.name))?;
        if !matches_hql_type(value, spec) {
            return Err(format!(
                "parameter {} must match {}",
                spec.name, spec.hql_type
            ));
        }
    }
    Ok(())
}

fn matches_hql_type(value: &Value, spec: &ParamSpec) -> bool {
    match spec.hql_type {
        "String" | "Date" | "ID" => value.is_string(),
        "I64" => value.as_i64().is_some(),
        "F64" => value.as_f64().is_some(),
        "[String]" => value
            .as_array()
            .is_some_and(|values| values.iter().all(Value::is_string)),
        "[F64]" => value
            .as_array()
            .is_some_and(|values| values.iter().all(|value| value.as_f64().is_some())),
        _ => false,
    }
}

fn parameter_names(params: &Value) -> Vec<String> {
    params
        .as_object()
        .map(|values| {
            values
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        })
        .unwrap_or_default()
}

fn hash_request(params: &Value) -> (String, u64) {
    let body = serde_json::to_vec(params).unwrap_or_default();
    let digest = Sha256::digest(body);
    let mut prefix = [0_u8; 8];
    prefix.copy_from_slice(&digest[..8]);
    (format!("{digest:x}"), u64::from_be_bytes(prefix))
}

async fn admin_metrics(State(state): State<AppState>) -> impl IntoResponse {
    axum::Json(state.metrics.snapshot().await)
}

async fn admin_registry() -> impl IntoResponse {
    axum::Json(manifest())
}

async fn admin_reset(State(state): State<AppState>) -> impl IntoResponse {
    let mut store = state.store.lock().await;
    store.clear();
    store.seed(state.scenario);
    drop(store);
    state.metrics.reset().await;
    axum::Json(json!({"ok":true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use clap::Parser;
    use http::Request;
    use tower::ServiceExt;

    fn state() -> AppState {
        AppState::new(&Config::parse_from(["mock"])).unwrap()
    }

    #[tokio::test]
    async fn post_health_matches_helix_client_transport() {
        let response = data_router(state())
            .oneshot(
                Request::post("/health")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn retired_rust_only_endpoints_fail_closed() {
        for endpoint in [
            "updateMemoryValidUntil",
            "updateMemoryContent",
            "linkMemoryToContext",
            "getEntitiesForMemory",
            "searchEntities",
            "getRecentRelations",
        ] {
            let response = data_router(state())
                .oneshot(
                    Request::post(format!("/{endpoint}"))
                        .header("content-type", "application/json")
                        .body(Body::from("{}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{endpoint}");
        }
    }

    #[tokio::test]
    async fn context_name_lookup_is_a_registered_graph_query() {
        let response = data_router(state())
            .oneshot(
                Request::post("/getContextByName")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"missing-context"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn missing_first_is_non_200_graph_error() {
        let response = data_router(state())
            .oneshot(
                Request::post("/getMemory")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"memory_id":"absent"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("GRAPH_ERROR"));
    }

    #[tokio::test]
    async fn literal_is_wrapped_in_data() {
        let response = data_router(state())
            .oneshot(
                Request::post("/getHelixirSchemaVersion")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({"data":"helixir-rbac-moirai-v4"})
        );
    }
}
