//! Native supervisor handlers for typed settings and backup administration.

use axum::Json;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use super::supervisor::{SupervisorProblem, supervisor_problem};

type Problem = (StatusCode, Json<SupervisorProblem>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsMutationReceipt {
    pub apply: super::settings::SettingsApplyResult,
    pub reload: super::settings_reload::ReloadReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupIdRequest {
    pub backup_id: String,
}

pub(super) async fn settings() -> Json<super::settings::SettingsSnapshot> {
    Json(super::settings::load())
}

pub(super) async fn apply_settings(
    Json(patch): Json<super::settings::SettingsPatch>,
) -> Result<Json<SettingsMutationReceipt>, Problem> {
    blocking(move || {
        let apply = super::settings::apply(&patch)?;
        let reload = if apply.changed {
            super::settings_reload::reload()?
        } else {
            super::settings_reload::ReloadReceipt {
                signalled_processes: 0,
                failed_signals: 0,
                restart_required: Vec::new(),
            }
        };
        Ok(SettingsMutationReceipt { apply, reload })
    })
    .await
    .map(Json)
}

pub(super) async fn backups() -> Json<super::backups::BackupInventory> {
    Json(super::backups::inventory())
}

pub(super) async fn create_backup() -> Result<Json<super::backups::BackupReceipt>, Problem> {
    blocking(super::backups::create).await.map(Json)
}

pub(super) async fn verify_backup(
    Json(request): Json<BackupIdRequest>,
) -> Result<Json<super::backups::BackupReceipt>, Problem> {
    blocking(move || super::backups::verify(&request.backup_id))
        .await
        .map(Json)
}

pub(super) async fn restore_backup(
    Json(request): Json<super::backups::RestoreRequest>,
) -> Result<Json<super::backups::BackupReceipt>, Problem> {
    super::backups::restore_verified(request)
        .await
        .map(Json)
        .map_err(|error| {
            supervisor_problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "admin_operation_rejected",
                error.to_string(),
            )
        })
}

async fn blocking<T, F>(operation: F) -> Result<T, Problem>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| {
            supervisor_problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                "admin_worker_failed",
                error.to_string(),
            )
        })?
        .map_err(|error| {
            supervisor_problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "admin_operation_rejected",
                error.to_string(),
            )
        })
}
