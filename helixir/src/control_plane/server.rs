//! Axum host for the versioned control-plane API and compiled frontend.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, ensure};
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::{get, post};
use axum::{Json, Router};
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use super::auth::{require_same_origin, require_token};
use super::dto::ApiProblem;
use super::graph::{MemoryFieldRequest, load_memory_field};
use super::graph_snapshot::CategoryGraphCache;
use super::moirai::load_moirai;
use super::response_security::{resolve_assets, security_header, validate_bind};
use super::stats::{load_access, load_overview, load_system, resolve_operator_id};
use super::supervisor::SupervisorClient;
use super::{
    AccessProjection, ControlPlaneConfig, ControlPlaneMeta, DiscoveryResponse,
    MemoryFieldProjection, MoiraiProjection, OverviewStats, SystemProjection,
};

#[derive(Clone)]
pub(super) struct AppState {
    pub session_token: Arc<str>,
    pub actor_id: Arc<str>,
    pub db: Arc<crate::db::HelixClient>,
    pub admin_required: bool,
    pub containerized: bool,
    pub supervisor: Option<Arc<SupervisorClient>>,
    pub category_graph: CategoryGraphCache,
}

pub(super) async fn serve(config: ControlPlaneConfig) -> anyhow::Result<()> {
    validate_bind(&config)?;
    let assets = resolve_assets(config.assets.as_deref())?;
    let index = assets.join("index.html");
    ensure!(
        index.is_file(),
        "web frontend is not built at {}",
        index.display()
    );

    let token_path = super::session::token_path(config.token_file.as_deref(), config.containerized);
    let token = super::session::load_token(&token_path, config.containerized)?;
    let runtime = crate::core::HelixirConfig::from_env();
    let db = Arc::new(crate::db::HelixClient::new(&runtime.host, runtime.port)?);
    let admin_required = config.containerized || admin_required(&db).await;
    let supervisor = SupervisorClient::from_env()?;
    let category_graph = CategoryGraphCache::default();
    category_graph.spawn(Arc::clone(&db));
    let state = AppState {
        session_token: Arc::from(token.as_str()),
        actor_id: Arc::from(resolve_operator_id()),
        db,
        admin_required,
        containerized: config.containerized,
        supervisor: supervisor.map(Arc::new),
        category_graph,
    };
    let api = Router::new()
        .route("/meta", get(meta))
        .route("/discovery", get(discovery))
        .route("/overview", get(overview))
        .route("/access", get(access))
        .route("/access/groups", post(super::admin::create_group))
        .route("/access/groups/add-user", post(super::admin::add_user))
        .route(
            "/access/groups/remove-user",
            post(super::admin::remove_user),
        )
        .route(
            "/access/groups/deactivate",
            post(super::admin::deactivate_group),
        )
        .route("/access/dedup", post(super::admin::create_dedup_group))
        .route(
            "/access/dedup/attach",
            post(super::admin::attach_dedup_group),
        )
        .route(
            "/access/dedup/detach",
            post(super::admin::detach_dedup_group),
        )
        .route(
            "/access/dedup/deactivate",
            post(super::admin::deactivate_dedup_group),
        )
        .route("/access/grants", post(super::admin::grant_role))
        .route("/access/revocations", post(super::admin::revoke_role))
        .route("/access/check", post(super::admin::check_access))
        .route("/access/agents/prune", post(super::admin::prune_agent))
        .route("/memory-field", get(memory_field))
        .route("/moirai", get(moirai))
        .route("/system", get(system))
        .route("/health", get(health))
        .route("/install/plan", post(build_install_plan))
        .route("/install/apply", post(apply_install_plan))
        .route("/install/operations", post(super::operations::start))
        .route(
            "/install/operations/{operation_id}",
            get(super::operations::status),
        )
        .route(
            "/install/operations/{operation_id}/events",
            get(super::operations::events),
        )
        .route(
            "/install/operations/{operation_id}/resume",
            post(super::operations::resume),
        )
        .route("/install/verify", post(verify_installation))
        .route("/operations/run", post(run_host_operation))
        .route(
            "/settings",
            get(super::host_admin::settings).post(super::host_admin::apply_settings),
        )
        .route("/backups", get(super::host_admin::backups))
        .route("/backups/create", post(super::host_admin::create_backup))
        .route("/backups/verify", post(super::host_admin::verify_backup))
        .route("/backups/restore", post(super::host_admin::restore_backup))
        .fallback(api_not_found)
        .method_not_allowed_fallback(api_method_not_allowed)
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(security_header("cache-control", "no-store"))
        .layer(middleware::from_fn(require_same_origin))
        .layer(middleware::from_fn_with_state(state.clone(), require_token));
    let static_files = ServeDir::new(&assets).fallback(ServeFile::new(index));
    let app = Router::new()
        .nest("/api/v1", api)
        .fallback_service(static_files)
        .with_state(state)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(security_header(
            "content-security-policy",
            "default-src 'self'; connect-src 'self'; font-src 'self'; img-src 'self'; object-src 'none'; script-src 'self'; style-src 'self'; base-uri 'none'; frame-ancestors 'none'",
        ))
        .layer(security_header("referrer-policy", "no-referrer"))
        .layer(security_header("x-content-type-options", "nosniff"))
        .layer(security_header("x-frame-options", "DENY"))
        .layer(security_header(
            "permissions-policy",
            "camera=(), geolocation=(), microphone=()",
        ))
        .layer(security_header(
            "cross-origin-resource-policy",
            "same-origin",
        ));

    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("bind web control plane at {}", config.bind))?;
    let address = listener.local_addr()?;
    let browser_host = if address.ip().is_unspecified() {
        "127.0.0.1".to_string()
    } else {
        address.ip().to_string()
    };
    let url = format!("http://{browser_host}:{}/#token={token}", address.port());
    eprintln!("Helixir web control plane: {url}");
    eprintln!("Helixir browser token: {}", token_path.display());
    if config.open_browser
        && let Err(error) = open::that_detached(&url)
    {
        tracing::warn!(%error, "could not open the system browser");
    }
    axum::serve(listener, app)
        .await
        .context("serve web control plane")
}

async fn admin_required(db: &Arc<crate::db::HelixClient>) -> bool {
    match crate::core::RbacManager::new(Arc::clone(db))
        .snapshot()
        .await
    {
        Ok(policy) => policy.enabled,
        Err(error) => {
            let required_by_manifest = std::env::var_os("HOME")
                .map(PathBuf::from)
                .and_then(|home| {
                    crate::installer::manifest::read(&home.join(".helixir/install.json"))
                        .ok()
                        .flatten()
                })
                .and_then(|manifest| manifest.rbac)
                .is_some_and(|rbac| rbac.enabled);
            if required_by_manifest {
                tracing::warn!(%error, "RBAC graph unavailable; keeping web control plane fail-closed");
            }
            required_by_manifest
        }
    }
}

async fn meta(State(state): State<AppState>) -> Json<ControlPlaneMeta> {
    Json(ControlPlaneMeta {
        product: "Helixir",
        version: env!("CARGO_PKG_VERSION"),
        api_version: "v1",
        phase: if state.admin_required {
            "admin"
        } else {
            "bootstrap"
        },
        transport: if state.containerized {
            "container-network"
        } else {
            "loopback"
        },
        runtime: if state.containerized {
            "control-plane-container"
        } else {
            "native-development"
        },
        host_operations_available: !state.containerized || state.supervisor.is_some(),
    })
}

async fn discovery(
    State(state): State<AppState>,
) -> Result<Json<DiscoveryResponse>, (StatusCode, Json<ApiProblem>)> {
    let discovered = match require_host_operations(&state)? {
        Some(supervisor) => supervisor.discovery().await.map_err(supervisor_error)?,
        None => crate::installer::native::detect_system_state().await,
    };
    Ok(Json(DiscoveryResponse::from_state(discovered)))
}

async fn overview(State(state): State<AppState>) -> Json<OverviewStats> {
    Json(load_overview(&state.db, &state.actor_id).await)
}

async fn access(
    State(state): State<AppState>,
) -> Result<Json<AccessProjection>, (StatusCode, Json<ApiProblem>)> {
    load_access(&state.db, &state.actor_id)
        .await
        .map(Json)
        .ok_or_else(projection_unavailable)
}

async fn memory_field(
    State(state): State<AppState>,
    Query(params): Query<MemoryFieldParams>,
) -> Result<Json<MemoryFieldProjection>, (StatusCode, Json<ApiProblem>)> {
    load_memory_field(
        &state.category_graph,
        &state.db,
        &state.actor_id,
        MemoryFieldRequest {
            group: params.group.as_deref(),
            identity: params.identity.as_deref(),
            focus: params.focus.as_deref(),
            query: params.query.as_deref(),
            page: params.page.unwrap_or(1),
        },
    )
    .await
    .map(Json)
    .ok_or_else(projection_unavailable)
}

#[derive(Debug, serde::Deserialize)]
struct MemoryFieldParams {
    group: Option<String>,
    identity: Option<String>,
    focus: Option<String>,
    query: Option<String>,
    page: Option<usize>,
}

async fn moirai(
    State(state): State<AppState>,
) -> Result<Json<MoiraiProjection>, (StatusCode, Json<ApiProblem>)> {
    load_moirai(&state.db, &state.actor_id)
        .await
        .map(Json)
        .ok_or_else(projection_unavailable)
}

async fn system(
    State(state): State<AppState>,
) -> Result<Json<SystemProjection>, (StatusCode, Json<ApiProblem>)> {
    load_system(&state.db, &state.actor_id)
        .await
        .map(Json)
        .ok_or_else(projection_unavailable)
}

async fn health(
    State(state): State<AppState>,
) -> Result<Json<crate::agents::hygieia::HealthSnapshot>, (StatusCode, Json<ApiProblem>)> {
    let snapshot = match require_host_operations(&state)? {
        Some(supervisor) => supervisor.health().await.map_err(supervisor_error)?,
        None => crate::agents::hygieia::snapshot(40).await,
    };
    Ok(Json(snapshot))
}

async fn api_not_found() -> (StatusCode, Json<ApiProblem>) {
    (
        StatusCode::NOT_FOUND,
        Json(ApiProblem {
            code: "api_route_not_found",
            message: "the requested control-plane API route does not exist".to_string(),
        }),
    )
}

async fn api_method_not_allowed() -> (StatusCode, Json<ApiProblem>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(ApiProblem {
            code: "api_method_not_allowed",
            message: "the requested method is not supported by this API route".to_string(),
        }),
    )
}

fn projection_unavailable() -> (StatusCode, Json<ApiProblem>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ApiProblem {
            code: "projection_unavailable",
            message: "the graph projection is temporarily unavailable".to_string(),
        }),
    )
}

async fn build_install_plan(
    State(state): State<AppState>,
    Json(options): Json<crate::installer::InstallOptions>,
) -> Result<Json<crate::installer::InstallPlan>, (StatusCode, Json<ApiProblem>)> {
    let supervisor = require_host_operations(&state)?;
    if options.rbac.operator_id != state.actor_id.as_ref() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiProblem {
                code: "operator_mismatch",
                message: "the install operator must match the authenticated web actor".to_string(),
            }),
        ));
    }
    if let Some(supervisor) = supervisor {
        return supervisor
            .plan(&options)
            .await
            .map(Json)
            .map_err(supervisor_error);
    }
    crate::installer::service::InstallerService::default()
        .prepare(&options)
        .await
        .map(|prepared| Json(prepared.plan))
        .map_err(|error| {
            tracing::warn!(%error, "control-plane install plan rejected");
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ApiProblem {
                    code: error.code(),
                    message: "the selected installation options do not form a safe plan"
                        .to_string(),
                }),
            )
        })
}

async fn apply_install_plan(
    State(state): State<AppState>,
    Json(options): Json<crate::installer::InstallOptions>,
) -> Result<Json<crate::installer::InstallReport>, (StatusCode, Json<ApiProblem>)> {
    if options.rbac.operator_id != state.actor_id.as_ref() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiProblem {
                code: "operator_mismatch",
                message: "the install operator must match the authenticated web actor".to_string(),
            }),
        ));
    }
    let Some(supervisor) = require_host_operations(&state)? else {
        return Err((
            StatusCode::NOT_IMPLEMENTED,
            Json(ApiProblem {
                code: "native_apply_unavailable",
                message: "web installation apply requires the authenticated host supervisor"
                    .to_string(),
            }),
        ));
    };
    supervisor
        .apply(&options)
        .await
        .map(Json)
        .map_err(supervisor_error)
}

async fn verify_installation(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiProblem>)> {
    let Some(supervisor) = require_host_operations(&state)? else {
        return Err((
            StatusCode::NOT_IMPLEMENTED,
            Json(ApiProblem {
                code: "native_verify_unavailable",
                message: "web verification requires the authenticated host supervisor".to_string(),
            }),
        ));
    };
    supervisor
        .verify()
        .await
        .map(Json)
        .map_err(supervisor_error)
}

async fn run_host_operation(
    State(state): State<AppState>,
    Json(request): Json<crate::installer::supervisor::HostOperation>,
) -> Result<Json<crate::installer::supervisor::HostOperationResult>, (StatusCode, Json<ApiProblem>)>
{
    if let Some(supervisor) = require_host_operations(&state)? {
        return supervisor
            .operation(&request)
            .await
            .map(Json)
            .map_err(supervisor_error);
    }
    tokio::task::spawn_blocking(move || crate::installer::supervisor::run_operation(&request))
        .await
        .map_err(|error| supervisor_error(error.into()))?
        .map(Json)
        .map_err(supervisor_error)
}

pub(super) fn require_host_operations(
    state: &AppState,
) -> Result<Option<&SupervisorClient>, (StatusCode, Json<ApiProblem>)> {
    if state.containerized && state.supervisor.is_none() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiProblem {
                code: "host_supervisor_unavailable",
                message: "host discovery and installation require the native Helixir supervisor"
                    .to_string(),
            }),
        ));
    }
    Ok(state.supervisor.as_deref())
}

pub(super) fn supervisor_error(error: anyhow::Error) -> (StatusCode, Json<ApiProblem>) {
    tracing::warn!(%error, "host supervisor request failed");
    let not_found = error.to_string().contains("host supervisor returned 404");
    (
        if not_found {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_GATEWAY
        },
        Json(ApiProblem {
            code: if not_found {
                "operation_not_found"
            } else {
                "host_supervisor_failed"
            },
            message: if not_found {
                "the requested operation journal does not exist".to_string()
            } else {
                "the native Helixir supervisor did not complete the request".to_string()
            },
        }),
    )
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
