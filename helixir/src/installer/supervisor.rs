//! Narrow authenticated host bridge for the isolated web control plane.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use anyhow::{Context, ensure};
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

use super::{InstallOptions, InstallPlan, Planner, SystemState};

/// Host supervisor settings. The listener may face the Docker bridge, so every
/// route requires the high-entropy token stored in a private file.
#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub bind: SocketAddr,
    pub token: String,
}

#[derive(Clone)]
pub(super) struct SupervisorState {
    pub(super) token: Arc<str>,
    pub(super) operations: super::operations::OperationStore,
}

/// Stable error envelope consumed by the container adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorProblem {
    pub code: String,
    pub message: String,
}

/// Explicit host mutations available to the admin-only browser. No arbitrary
/// command, path, environment or shell fragment crosses this boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostOperation {
    WatchOnce {},
    WatchStart { interval_secs: Option<u64> },
    WatchStop {},
    DaemonStart { user_id: String, interval_secs: u64 },
    DaemonStop {},
    ModelCheck {},
}

/// Bounded command receipt returned to the control plane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostOperationResult {
    pub operation: String,
    pub succeeded: bool,
    pub output: String,
}

/// Serve read-only host discovery and deterministic plan construction.
pub async fn serve(config: SupervisorConfig) -> anyhow::Result<()> {
    ensure!(config.token.len() >= 32, "supervisor token is too short");
    let operations =
        super::operations::OperationStore::open(super::operations::default_journal_dir())?;
    let state = SupervisorState {
        token: Arc::from(config.token),
        operations,
    };
    let app = Router::new()
        .route("/v1/discovery", get(discovery))
        .route("/v1/health", get(health))
        .route("/v1/install/plan", post(plan))
        .route(
            "/v1/install/apply",
            post(super::supervisor_operations::apply_legacy),
        )
        .route(
            "/v1/install/operations",
            post(super::supervisor_operations::start),
        )
        .route(
            "/v1/install/operations/{operation_id}",
            get(super::supervisor_operations::status),
        )
        .route(
            "/v1/install/operations/{operation_id}/events",
            get(super::supervisor_operations::events),
        )
        .route(
            "/v1/install/operations/{operation_id}/resume",
            post(super::supervisor_operations::resume),
        )
        .route("/v1/install/verify", post(verify))
        .route("/v1/operations/run", post(operation))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), authorize))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("bind host supervisor at {}", config.bind))?;
    eprintln!("Helixir host supervisor: {}", listener.local_addr()?);
    axum::serve(listener, app)
        .await
        .context("serve host supervisor")
}

async fn discovery() -> Json<SystemState> {
    Json(super::native::detect_system_state().await)
}

async fn health() -> Json<crate::agents::hygieia::HealthSnapshot> {
    Json(crate::agents::hygieia::snapshot(40).await)
}

async fn plan(
    Json(options): Json<InstallOptions>,
) -> Result<Json<InstallPlan>, (StatusCode, Json<SupervisorProblem>)> {
    let state = super::native::detect_system_state().await;
    Planner::build(&state, &options).map(Json).map_err(|error| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(SupervisorProblem {
                code: "unsafe_install_plan".to_string(),
                message: error.to_string(),
            }),
        )
    })
}

async fn verify() -> Result<Json<serde_json::Value>, (StatusCode, Json<SupervisorProblem>)> {
    tokio::task::spawn_blocking(run_doctor)
        .await
        .map_err(|error| {
            supervisor_problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                "doctor_worker_failed",
                error.to_string(),
            )
        })?
        .map(Json)
        .map_err(|error| {
            supervisor_problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "doctor_failed",
                error.to_string(),
            )
        })
}

async fn operation(
    Json(request): Json<HostOperation>,
) -> Result<Json<HostOperationResult>, (StatusCode, Json<SupervisorProblem>)> {
    tokio::task::spawn_blocking(move || run_operation(&request))
        .await
        .map_err(|error| {
            supervisor_problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                "operation_worker_failed",
                error.to_string(),
            )
        })?
        .map(Json)
        .map_err(|error| {
            supervisor_problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "operation_rejected",
                error.to_string(),
            )
        })
}

/// Run one typed, bounded host operation without invoking a shell.
pub fn run_operation(request: &HostOperation) -> anyhow::Result<HostOperationResult> {
    let mut args = Vec::new();
    let operation = match request {
        HostOperation::WatchOnce {} => {
            args.extend(["watch".to_string(), "run".to_string(), "--once".to_string()]);
            "watch_once"
        }
        HostOperation::WatchStart { interval_secs } => {
            args.extend(["watch".to_string(), "start".to_string()]);
            if let Some(interval) = interval_secs {
                ensure!(
                    (5..=86_400).contains(interval),
                    "watch interval must be 5..86400 seconds"
                );
                args.extend(["--interval".to_string(), interval.to_string()]);
            }
            "watch_start"
        }
        HostOperation::WatchStop {} => {
            args.extend(["watch".to_string(), "stop".to_string()]);
            "watch_stop"
        }
        HostOperation::DaemonStart {
            user_id,
            interval_secs,
        } => {
            ensure!(!user_id.trim().is_empty(), "daemon user id is required");
            ensure!(
                (30..=86_400).contains(interval_secs),
                "daemon interval must be 30..86400 seconds"
            );
            args.extend([
                "daemon".to_string(),
                "start".to_string(),
                "--user".to_string(),
                user_id.trim().to_string(),
                "--interval".to_string(),
                interval_secs.to_string(),
            ]);
            "daemon_start"
        }
        HostOperation::DaemonStop {} => {
            args.extend(["daemon".to_string(), "stop".to_string()]);
            "daemon_stop"
        }
        HostOperation::ModelCheck {} => {
            args.extend(["model".to_string(), "check".to_string()]);
            "model_check"
        }
    };
    let binary = std::env::current_exe().context("resolve Helixir supervisor executable")?;
    let output = Command::new(binary)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("run typed operation {operation}"))?;
    let merged = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(HostOperationResult {
        operation: operation.to_string(),
        succeeded: output.status.success(),
        output: merged
            .chars()
            .take(16_384)
            .collect::<String>()
            .trim()
            .to_string(),
    })
}

fn run_doctor() -> anyhow::Result<serde_json::Value> {
    let binary = std::env::current_exe().context("resolve Helixir supervisor executable")?;
    let output = Command::new(binary)
        .args(["doctor", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("run Helixir doctor")?;
    serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "doctor returned invalid JSON: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
    })
}

pub(super) fn supervisor_problem(
    status: StatusCode,
    code: &str,
    message: String,
) -> (StatusCode, Json<SupervisorProblem>) {
    (
        status,
        Json(SupervisorProblem {
            code: code.to_string(),
            message,
        }),
    )
}

async fn authorize(
    State(state): State<SupervisorState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(candidate) = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    if candidate.len() != state.token.len()
        || !bool::from(candidate.as_bytes().ct_eq(state.token.as_bytes()))
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(request).await)
}

/// Load the private supervisor token, creating it atomically on first use.
pub fn load_or_create_token(path: &Path) -> anyhow::Result<String> {
    if let Some(token) = read_token(path)? {
        return Ok(token);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create supervisor state dir {}", parent.display()))?;
    }
    let token = uuid::Uuid::new_v4().simple().to_string();
    match private_create(path) {
        Ok(mut file) => {
            file.write_all(token.as_bytes())?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            Ok(token)
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            read_token(path)?.context("supervisor token appeared concurrently but is invalid")
        }
        Err(error) => {
            Err(error).with_context(|| format!("create supervisor token {}", path.display()))
        }
    }
}

/// Default state-file location shared with the container secret mount.
pub fn default_token_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".helixir/run/control-plane.token")
}

fn read_token(path: &Path) -> anyhow::Result<Option<String>> {
    let value = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let token = value.trim().to_string();
    ensure!(token.len() >= 32, "supervisor token file is invalid");
    Ok(Some(token))
}

#[cfg(unix)]
fn private_create(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn private_create(path: &Path) -> std::io::Result<std::fs::File> {
    OpenOptions::new().create_new(true).write(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_stable_and_private() {
        let root = std::env::temp_dir().join(format!(
            "helixir-supervisor-token-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let path = root.join("token");
        let first = load_or_create_token(&path).unwrap();
        let second = load_or_create_token(&path).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 32);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn host_operation_contract_rejects_shell_shaped_fields() {
        let result = serde_json::from_value::<HostOperation>(serde_json::json!({
            "kind": "watch_stop",
            "command": "rm -rf /"
        }));
        assert!(result.is_err());
    }

    #[test]
    fn host_operation_validates_daemon_inputs_before_spawning() {
        assert!(
            run_operation(&HostOperation::DaemonStart {
                user_id: "".to_string(),
                interval_secs: 300,
            })
            .is_err()
        );
        assert!(
            run_operation(&HostOperation::DaemonStart {
                user_id: "codex".to_string(),
                interval_secs: 1,
            })
            .is_err()
        );
    }
}
