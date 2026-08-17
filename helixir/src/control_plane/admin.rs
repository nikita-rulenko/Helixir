//! Graph-backed RBAC mutations shared with the CLI domain service.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::core::{RbacManager, Role};

use super::dto::{
    AccessCheckRequest, AccessCheckResult, ApiProblem, DedupGroupMutation, DedupMembershipMutation,
    GroupMemberMutation, GroupMutation, GroupUserMutation, MutationReceipt, ResourceMutation,
    RoleMutation,
};
use super::server::AppState;

pub(super) async fn grant_role(
    State(state): State<AppState>,
    Json(request): Json<RoleMutation>,
) -> Result<Json<MutationReceipt>, (StatusCode, Json<ApiProblem>)> {
    let role = parse_role(&request.role)?;
    validate_scope(role, request.group_id.as_deref())?;
    RbacManager::new(Arc::clone(&state.db))
        .grant(
            request.subject_id.trim(),
            role,
            request.group_id.as_deref().map(str::trim),
            &state.actor_id,
        )
        .await
        .map_err(mutation_error)?;
    Ok(Json(MutationReceipt {
        ok: true,
        message: format!("granted {} to {}", role.label(), request.subject_id.trim()),
    }))
}

pub(super) async fn revoke_role(
    State(state): State<AppState>,
    Json(request): Json<RoleMutation>,
) -> Result<Json<MutationReceipt>, (StatusCode, Json<ApiProblem>)> {
    let role = parse_role(&request.role)?;
    validate_scope(role, request.group_id.as_deref())?;
    RbacManager::new(Arc::clone(&state.db))
        .revoke_as(
            request.subject_id.trim(),
            role,
            request.group_id.as_deref().map(str::trim),
            &state.actor_id,
        )
        .await
        .map_err(mutation_error)?;
    Ok(Json(MutationReceipt {
        ok: true,
        message: format!(
            "revoked {} from {}",
            role.label(),
            request.subject_id.trim()
        ),
    }))
}

pub(super) async fn create_group(
    State(state): State<AppState>,
    Json(request): Json<GroupMutation>,
) -> Result<Json<MutationReceipt>, (StatusCode, Json<ApiProblem>)> {
    let group_id = request.group_id.trim();
    let name = request.name.trim();
    if group_id.is_empty() || name.is_empty() {
        return Err(problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_group",
            "group id and name are required",
        ));
    }
    RbacManager::new(Arc::clone(&state.db))
        .create_group_as(group_id, name, request.description.trim(), &state.actor_id)
        .await
        .map_err(mutation_error)?;
    Ok(Json(MutationReceipt {
        ok: true,
        message: format!("group '{group_id}' created or updated"),
    }))
}

pub(super) async fn add_user(
    State(state): State<AppState>,
    Json(request): Json<GroupMemberMutation>,
) -> Result<Json<MutationReceipt>, (StatusCode, Json<ApiProblem>)> {
    let role = parse_role(&request.role)?;
    if matches!(role, Role::Admin) {
        return Err(problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_role_scope",
            "group membership requires groupadmin, moderator, worker, or viewer",
        ));
    }
    let (group_id, subject_id) = required_pair(
        &request.group_id,
        &request.subject_id,
        "group id and principal id are required",
    )?;
    RbacManager::new(Arc::clone(&state.db))
        .add_user_to_group(subject_id, group_id, role, &state.actor_id)
        .await
        .map_err(mutation_error)?;
    Ok(Json(receipt(format!(
        "added '{subject_id}' to '{group_id}' as {}",
        role.label()
    ))))
}

pub(super) async fn remove_user(
    State(state): State<AppState>,
    Json(request): Json<GroupUserMutation>,
) -> Result<Json<MutationReceipt>, (StatusCode, Json<ApiProblem>)> {
    let (group_id, subject_id) = required_pair(
        &request.group_id,
        &request.subject_id,
        "group id and principal id are required",
    )?;
    let revoked = RbacManager::new(Arc::clone(&state.db))
        .remove_user_from_group(subject_id, group_id, &state.actor_id)
        .await
        .map_err(mutation_error)?;
    Ok(Json(receipt(format!(
        "removed '{subject_id}' from '{group_id}' ({} role assignments revoked)",
        revoked.len()
    ))))
}

pub(super) async fn deactivate_group(
    State(state): State<AppState>,
    Json(request): Json<ResourceMutation>,
) -> Result<Json<MutationReceipt>, (StatusCode, Json<ApiProblem>)> {
    let id = required(&request.id, "group id is required")?;
    RbacManager::new(Arc::clone(&state.db))
        .deactivate_group_as(id, &state.actor_id)
        .await
        .map_err(mutation_error)?;
    Ok(Json(receipt(format!("group '{id}' deactivated"))))
}

pub(super) async fn create_dedup_group(
    State(state): State<AppState>,
    Json(request): Json<DedupGroupMutation>,
) -> Result<Json<MutationReceipt>, (StatusCode, Json<ApiProblem>)> {
    let (id, name) = required_pair(
        &request.dedup_group_id,
        &request.name,
        "dedup federation id and name are required",
    )?;
    RbacManager::new(Arc::clone(&state.db))
        .create_dedup_group_as(id, name, request.description.trim(), &state.actor_id)
        .await
        .map_err(mutation_error)?;
    Ok(Json(receipt(format!(
        "dedup federation '{id}' created or updated"
    ))))
}

pub(super) async fn attach_dedup_group(
    State(state): State<AppState>,
    Json(request): Json<DedupMembershipMutation>,
) -> Result<Json<MutationReceipt>, (StatusCode, Json<ApiProblem>)> {
    let (group_id, dedup_group_id) = required_pair(
        &request.group_id,
        &request.dedup_group_id,
        "group id and dedup federation id are required",
    )?;
    let linked = RbacManager::new(Arc::clone(&state.db))
        .attach_group_to_dedup_as(group_id, dedup_group_id, &state.actor_id)
        .await
        .map_err(mutation_error)?;
    Ok(Json(receipt(format!(
        "attached '{group_id}' to '{dedup_group_id}' ({linked} historical memories linked)"
    ))))
}

pub(super) async fn detach_dedup_group(
    State(state): State<AppState>,
    Json(request): Json<DedupMembershipMutation>,
) -> Result<Json<MutationReceipt>, (StatusCode, Json<ApiProblem>)> {
    let group_id = required(&request.group_id, "group id is required")?;
    RbacManager::new(Arc::clone(&state.db))
        .detach_group_from_dedup_as(group_id, &state.actor_id)
        .await
        .map_err(mutation_error)?;
    Ok(Json(receipt(format!(
        "detached '{group_id}'; historical access retained"
    ))))
}

pub(super) async fn deactivate_dedup_group(
    State(state): State<AppState>,
    Json(request): Json<ResourceMutation>,
) -> Result<Json<MutationReceipt>, (StatusCode, Json<ApiProblem>)> {
    let id = required(&request.id, "dedup federation id is required")?;
    RbacManager::new(Arc::clone(&state.db))
        .deactivate_dedup_group_as(id, &state.actor_id)
        .await
        .map_err(mutation_error)?;
    Ok(Json(receipt(format!(
        "dedup federation '{id}' deactivated"
    ))))
}

pub(super) async fn check_access(
    State(state): State<AppState>,
    Json(request): Json<AccessCheckRequest>,
) -> Result<Json<AccessCheckResult>, (StatusCode, Json<ApiProblem>)> {
    let subject = required(&request.subject_id, "principal id is required")?;
    let action = request.action.trim().to_ascii_lowercase();
    let owner = request
        .owner_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let policy = RbacManager::new(Arc::clone(&state.db))
        .snapshot()
        .await
        .map_err(mutation_error)?;
    let allowed = match action.as_str() {
        "read" => policy
            .readable_users(subject)
            .is_none_or(|users| owner.is_none_or(|target| users.contains(target))),
        "write" => owner
            .map(|target| policy.can_write_owner(subject, target))
            .unwrap_or_else(|| policy.can_write(subject)),
        _ => {
            return Err(problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_action",
                "action must be read or write",
            ));
        }
    };
    let target = owner.map(str::to_string);
    Ok(Json(AccessCheckResult {
        allowed,
        subject_id: subject.to_string(),
        action: action.clone(),
        owner_id: target.clone(),
        explanation: format!(
            "{subject} is {} to {action} {}",
            if allowed { "allowed" } else { "denied" },
            target.as_deref().unwrap_or("the permitted scope")
        ),
    }))
}

pub(super) async fn prune_agent(
    State(state): State<AppState>,
    Json(request): Json<ResourceMutation>,
) -> Result<Json<MutationReceipt>, (StatusCode, Json<ApiProblem>)> {
    let agent_id = required(&request.id, "agent id is required")?;
    RbacManager::new(Arc::clone(&state.db))
        .authorize_admin_surface(&state.actor_id)
        .await
        .map_err(mutation_error)?;
    state
        .db
        .execute_query::<serde_json::Value, _>(
            "dropPresenceByAgentId",
            &serde_json::json!({"agent_id": agent_id}),
        )
        .await
        .map_err(|error| mutation_error(error.into()))?;
    Ok(Json(receipt(format!(
        "pruned junk presence row for '{agent_id}'"
    ))))
}

fn receipt(message: String) -> MutationReceipt {
    MutationReceipt { ok: true, message }
}

fn required<'a>(value: &'a str, message: &str) -> Result<&'a str, (StatusCode, Json<ApiProblem>)> {
    let value = value.trim();
    if value.is_empty() {
        Err(problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "missing_value",
            message,
        ))
    } else {
        Ok(value)
    }
}

fn required_pair<'a>(
    first: &'a str,
    second: &'a str,
    message: &str,
) -> Result<(&'a str, &'a str), (StatusCode, Json<ApiProblem>)> {
    Ok((required(first, message)?, required(second, message)?))
}

fn parse_role(raw: &str) -> Result<Role, (StatusCode, Json<ApiProblem>)> {
    Role::parse(raw)
        .filter(|role| !role.is_legacy())
        .ok_or_else(|| {
            problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_role",
                "role must be admin, groupadmin, moderator, worker, or viewer",
            )
        })
}

fn validate_scope(
    role: Role,
    group_id: Option<&str>,
) -> Result<(), (StatusCode, Json<ApiProblem>)> {
    let has_group = group_id.is_some_and(|group| !group.trim().is_empty());
    if matches!(role, Role::Admin) == has_group {
        return Err(problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_role_scope",
            "admin is global; every other role requires a concrete group",
        ));
    }
    Ok(())
}

fn mutation_error(error: anyhow::Error) -> (StatusCode, Json<ApiProblem>) {
    tracing::warn!(%error, "control-plane RBAC mutation rejected");
    problem(
        StatusCode::UNPROCESSABLE_ENTITY,
        "rbac_mutation_rejected",
        &error.to_string(),
    )
}

fn problem(
    status: StatusCode,
    code: &'static str,
    message: &str,
) -> (StatusCode, Json<ApiProblem>) {
    (
        status,
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
    fn role_scope_contract_is_explicit() {
        assert!(validate_scope(Role::Admin, None).is_ok());
        assert!(validate_scope(Role::Admin, Some("default")).is_err());
        assert!(validate_scope(Role::Moderator, Some("dev")).is_ok());
        assert!(validate_scope(Role::Viewer, None).is_err());
    }
}
