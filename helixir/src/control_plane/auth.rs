//! Persistent bearer authorization for the local admin control plane.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use subtle::ConstantTimeEq;

use super::server::AppState;

pub(super) async fn require_token(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if !token_matches(request.headers(), &state.session_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    match crate::core::RbacManager::new(state.db.clone())
        .snapshot()
        .await
    {
        Ok(policy) if policy.enabled && !admin_allowed(&policy, &state.actor_id) => {
            return Err(StatusCode::FORBIDDEN);
        }
        Ok(_) => {}
        Err(error) if state.admin_required => {
            tracing::warn!(%error, "control-plane admin authorization unavailable");
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        Err(error) => {
            tracing::debug!(%error, "RBAC is not initialized; setup session remains active")
        }
    }
    Ok(next.run(request).await)
}

fn admin_allowed(policy: &crate::core::RbacPolicy, actor: &str) -> bool {
    policy.enabled && policy.is_admin(actor)
}

fn token_matches(headers: &HeaderMap, expected: &str) -> bool {
    let Some(candidate) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return false;
    };
    candidate.len() == expected.len() && bool::from(candidate.as_bytes().ct_eq(expected.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn bearer_token_is_exact_and_case_sensitive() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer exact-token"),
        );
        assert!(token_matches(&headers, "exact-token"));
        assert!(!token_matches(&headers, "Exact-token"));
    }

    #[test]
    fn active_ui_rejects_groupadmin_and_accepts_only_global_admin() {
        use crate::core::{RbacPolicy, Role};

        let mut policy = RbacPolicy {
            enabled: true,
            ..RbacPolicy::default()
        };
        policy.upsert_group("engineering", "Engineering");
        policy
            .assign_group("lead", "engineering", Role::GroupAdmin)
            .unwrap();
        policy.assign_global("operator", Role::Admin);

        assert!(!admin_allowed(&policy, "lead"));
        assert!(admin_allowed(&policy, "operator"));
    }

    #[test]
    fn every_non_admin_role_is_denied_the_admin_ui() {
        use crate::core::{RbacPolicy, Role};

        let mut policy = RbacPolicy {
            enabled: true,
            ..RbacPolicy::default()
        };
        policy.upsert_group("engineering", "Engineering");
        policy.assign_global("admin", Role::Admin);
        for (subject, role) in [
            ("group-admin", Role::GroupAdmin),
            ("moderator", Role::Moderator),
            ("worker", Role::Worker),
            ("viewer", Role::Viewer),
        ] {
            policy.assign_group(subject, "engineering", role).unwrap();
            assert!(!admin_allowed(&policy, subject), "{subject} must be denied");
        }
        assert!(!admin_allowed(&policy, "unassigned"));
        assert!(admin_allowed(&policy, "admin"));
    }
}
