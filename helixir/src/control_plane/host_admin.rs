//! Global-admin HTTP adapters for settings and the native backup vault.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use super::dto::ApiProblem;
use super::server::{AppState, require_host_operations, supervisor_error};

type Problem = (StatusCode, Json<ApiProblem>);

pub(super) async fn settings(
    State(state): State<AppState>,
) -> Result<Json<crate::installer::settings::SettingsSnapshot>, Problem> {
    match require_host_operations(&state)? {
        Some(supervisor) => supervisor
            .settings()
            .await
            .map(Json)
            .map_err(supervisor_error),
        None => Ok(Json(crate::installer::settings::load())),
    }
}

pub(super) async fn apply_settings(
    State(state): State<AppState>,
    Json(patch): Json<crate::installer::settings::SettingsPatch>,
) -> Result<Json<crate::installer::supervisor_admin::SettingsMutationReceipt>, Problem> {
    match require_host_operations(&state)? {
        Some(supervisor) => supervisor
            .apply_settings(&patch)
            .await
            .map(Json)
            .map_err(supervisor_error),
        None => blocking(move || {
            let apply = crate::installer::settings::apply(&patch)?;
            let reload = crate::installer::settings_reload::reload()?;
            Ok(crate::installer::supervisor_admin::SettingsMutationReceipt { apply, reload })
        })
        .await
        .map(Json),
    }
}

pub(super) async fn backups(
    State(state): State<AppState>,
) -> Result<Json<crate::installer::backups::BackupInventory>, Problem> {
    match require_host_operations(&state)? {
        Some(supervisor) => supervisor
            .backups()
            .await
            .map(Json)
            .map_err(supervisor_error),
        None => Ok(Json(crate::installer::backups::inventory())),
    }
}

pub(super) async fn create_backup(
    State(state): State<AppState>,
) -> Result<Json<crate::installer::backups::BackupReceipt>, Problem> {
    match require_host_operations(&state)? {
        Some(supervisor) => supervisor
            .create_backup()
            .await
            .map(Json)
            .map_err(supervisor_error),
        None => blocking(crate::installer::backups::create).await.map(Json),
    }
}

pub(super) async fn verify_backup(
    State(state): State<AppState>,
    Json(request): Json<crate::installer::supervisor_admin::BackupIdRequest>,
) -> Result<Json<crate::installer::backups::BackupReceipt>, Problem> {
    match require_host_operations(&state)? {
        Some(supervisor) => supervisor
            .verify_backup(&request)
            .await
            .map(Json)
            .map_err(supervisor_error),
        None => blocking(move || crate::installer::backups::verify(&request.backup_id))
            .await
            .map(Json),
    }
}

pub(super) async fn restore_backup(
    State(state): State<AppState>,
    Json(request): Json<crate::installer::backups::RestoreRequest>,
) -> Result<Json<crate::installer::backups::BackupReceipt>, Problem> {
    match require_host_operations(&state)? {
        Some(supervisor) => supervisor
            .restore_backup(&request)
            .await
            .map(Json)
            .map_err(supervisor_error),
        None => crate::installer::backups::restore_verified(request)
            .await
            .map(Json)
            .map_err(internal_problem),
    }
}

async fn blocking<T, F>(operation: F) -> Result<T, Problem>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| internal_problem(error.into()))?
        .map_err(internal_problem)
}

fn internal_problem(error: anyhow::Error) -> Problem {
    tracing::warn!(%error, "native admin operation rejected");
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(ApiProblem {
            code: "admin_operation_rejected",
            message: "the native host rejected the requested administration operation".to_string(),
        }),
    )
}
