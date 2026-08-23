//! Atomic administrator placement for principals waiting in `onboarding`.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::core::rbac::{ClientWorkspaceOnboarding, ClientWorkspaceOnboardingReport};
use crate::core::{RbacManager, Role};

use super::dto::{ApiProblem, OnboardingPlacementMutation};
use super::server::AppState;

pub(super) async fn assign(
    State(state): State<AppState>,
    Json(request): Json<OnboardingPlacementMutation>,
) -> Result<Json<ClientWorkspaceOnboardingReport>, (StatusCode, Json<ApiProblem>)> {
    let role = placement_role(request.role.trim())?;
    let placement = ClientWorkspaceOnboarding {
        principal_id: request.principal_id.trim().to_string(),
        group_id: request.group_id.trim().to_string(),
        group_name: None,
        group_description: String::new(),
        role,
        keep_onboarding: false,
    };
    let report = RbacManager::new(Arc::clone(&state.db))
        .onboard_client_to_workspace_as(&placement, &state.actor_id)
        .await
        .map_err(rejected)?;
    Ok(Json(report))
}

fn placement_role(raw: &str) -> Result<Role, (StatusCode, Json<ApiProblem>)> {
    Role::parse(raw)
        .filter(|role| {
            matches!(
                role,
                Role::GroupAdmin | Role::Moderator | Role::Worker | Role::Viewer
            )
        })
        .ok_or_else(invalid_role)
}

fn invalid_role() -> (StatusCode, Json<ApiProblem>) {
    problem(
        "invalid_onboarding_role",
        "role must be groupadmin, moderator, worker, or viewer",
    )
}

fn rejected(error: anyhow::Error) -> (StatusCode, Json<ApiProblem>) {
    tracing::warn!(%error, "control-plane client placement rejected");
    problem(
        "onboarding_placement_rejected",
        "the graph-backed onboarding workflow rejected this placement",
    )
}

fn problem(code: &'static str, message: &str) -> (StatusCode, Json<ApiProblem>) {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(ApiProblem {
            code,
            message: message.to_string(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_surface_accepts_only_group_scoped_roles() {
        for role in ["groupadmin", "moderator", "worker", "viewer"] {
            assert!(placement_role(role).is_ok());
        }
        for role in ["admin", "teamlead", "unknown"] {
            assert!(placement_role(role).is_err());
        }
    }
}
