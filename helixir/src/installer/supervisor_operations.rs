//! Journaled installation endpoints owned by the native host supervisor.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;

use super::operations::{JournalObserver, OperationEventBatch, OperationSnapshot};
use super::supervisor::{SupervisorProblem, SupervisorState, supervisor_problem};
use super::{InstallOptions, InstallReport};

type Problem = (StatusCode, Json<SupervisorProblem>);

#[derive(Debug, Deserialize)]
pub(super) struct EventQuery {
    #[serde(default)]
    after: u64,
}

pub(super) async fn start(
    State(state): State<SupervisorState>,
    Json(options): Json<InstallOptions>,
) -> Result<Json<OperationSnapshot>, Problem> {
    let plan = rebuild_plan(&options).await?;
    let snapshot = state.operations.create(plan).map_err(operation_problem)?;
    spawn(state.operations, snapshot.operation_id.clone(), options);
    Ok(Json(snapshot))
}

pub(super) async fn resume(
    State(state): State<SupervisorState>,
    Path(operation_id): Path<String>,
    Json(options): Json<InstallOptions>,
) -> Result<Json<OperationSnapshot>, Problem> {
    let plan = rebuild_plan(&options).await?;
    let snapshot = state
        .operations
        .prepare_resume(&operation_id, &plan)
        .map_err(operation_problem)?;
    spawn(state.operations, operation_id, options);
    Ok(Json(snapshot))
}

pub(super) async fn status(
    State(state): State<SupervisorState>,
    Path(operation_id): Path<String>,
) -> Result<Json<OperationSnapshot>, Problem> {
    state
        .operations
        .get(&operation_id)
        .map(Json)
        .map_err(operation_problem)
}

pub(super) async fn events(
    State(state): State<SupervisorState>,
    Path(operation_id): Path<String>,
    Query(query): Query<EventQuery>,
) -> Result<Json<OperationEventBatch>, Problem> {
    state
        .operations
        .events_after(&operation_id, query.after)
        .map(Json)
        .map_err(operation_problem)
}

/// Compatibility endpoint for older clients. New browser clients use the journaled API.
pub(super) async fn apply_legacy(
    Json(options): Json<InstallOptions>,
) -> Result<Json<InstallReport>, Problem> {
    rebuild_plan(&options).await?;
    let observer = |_event| {};
    tokio::task::spawn_blocking(move || super::operation_worker::run(&options, &observer))
        .await
        .map_err(|error| {
            supervisor_problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                "install_worker_failed",
                error.to_string(),
            )
        })?
        .map(Json)
        .map_err(|error| {
            supervisor_problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                "install_apply_failed",
                error.to_string(),
            )
        })
}

async fn rebuild_plan(options: &InstallOptions) -> Result<super::InstallPlan, Problem> {
    super::service::InstallerService::default()
        .prepare(options)
        .await
        .map(|prepared| prepared.plan)
        .map_err(|error| {
            supervisor_problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                error.code(),
                "the selected installation options do not form a safe plan".to_string(),
            )
        })
}

fn spawn(store: super::operations::OperationStore, operation_id: String, options: InstallOptions) {
    tokio::task::spawn_blocking(move || {
        if let Err(error) = store.mark_running(&operation_id) {
            tracing::error!(%error, %operation_id, "start journaled install operation");
            return;
        }
        let observer = JournalObserver::new(store.clone(), operation_id.clone());
        match super::operation_worker::run(&options, &observer) {
            Ok(report) => {
                if let Err(error) = store.finish(&operation_id, Some(report), None) {
                    tracing::error!(%error, %operation_id, "finish journaled install operation");
                }
            }
            Err(error) => {
                if let Err(persist_error) =
                    store.finish(&operation_id, None, Some(&error.to_string()))
                {
                    tracing::error!(%persist_error, %operation_id, "persist failed install operation");
                }
            }
        }
    });
}

fn operation_problem(error: anyhow::Error) -> Problem {
    let message = error.to_string();
    let (status, code) = if message.contains("was not found") {
        (StatusCode::NOT_FOUND, "operation_not_found")
    } else if message.contains("already active") {
        (StatusCode::CONFLICT, "operation_active")
    } else {
        (StatusCode::UNPROCESSABLE_ENTITY, "operation_rejected")
    };
    supervisor_problem(status, code, message)
}
