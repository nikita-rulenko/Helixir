//! Persistent bearer authorization for the local admin control plane.

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use subtle::ConstantTimeEq;

use super::dto::ApiProblem;
use super::server::AppState;

type AuthProblem = (StatusCode, Json<ApiProblem>);

pub(super) async fn require_token(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AuthProblem> {
    if !token_matches(request.headers(), &state.session_token) {
        return Err(problem(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "a valid control-plane bearer token is required",
        ));
    }
    match crate::core::RbacManager::new(state.db.clone())
        .snapshot()
        .await
    {
        Ok(policy) if policy.enabled && !admin_allowed(&policy, &state.actor_id) => {
            return Err(problem(
                StatusCode::FORBIDDEN,
                "global_admin_required",
                "the control plane is restricted to graph-backed global admins",
            ));
        }
        Ok(_) => {}
        Err(error) if state.admin_required => {
            tracing::warn!(%error, "control-plane admin authorization unavailable");
            return Err(problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "authorization_unavailable",
                "graph-backed authorization is temporarily unavailable",
            ));
        }
        Err(error) => {
            tracing::debug!(%error, "RBAC is not initialized; setup session remains active")
        }
    }
    Ok(next.run(request).await)
}

/// Reject browser requests initiated by another origin. Non-browser clients
/// without Origin/Sec-Fetch-Site remain supported, but still need the bearer
/// token and graph-backed global-admin authorization.
pub(super) async fn require_same_origin(
    request: Request,
    next: Next,
) -> Result<Response, AuthProblem> {
    if !same_origin(request.headers()) {
        return Err(problem(
            StatusCode::FORBIDDEN,
            "cross_origin_request_denied",
            "control-plane requests must originate from this Helixir host",
        ));
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

fn same_origin(headers: &HeaderMap) -> bool {
    if headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("cross-site"))
    {
        return false;
    }
    let Some(origin) = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    let Some(host) = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    origin_matches_host(origin, host)
}

fn origin_matches_host(origin: &str, host: &str) -> bool {
    let Ok(origin) = url::Url::parse(origin) else {
        return false;
    };
    if !matches!(origin.scheme(), "http" | "https")
        || origin.username() != ""
        || origin.password().is_some()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        return false;
    }
    let Ok(authority) = host.parse::<axum::http::uri::Authority>() else {
        return false;
    };
    let Some(origin_host) = origin.host_str() else {
        return false;
    };
    let origin_port = origin.port_or_known_default();
    let host_port = authority.port_u16().or_else(|| match origin.scheme() {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    });
    authority
        .host()
        .trim_matches(['[', ']'])
        .eq_ignore_ascii_case(origin_host.trim_matches(['[', ']']))
        && host_port == origin_port
}

fn problem(status: StatusCode, code: &'static str, message: &str) -> AuthProblem {
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

    #[test]
    fn browser_origin_must_match_the_request_host() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::HOST,
            HeaderValue::from_static("127.0.0.1:6971"),
        );
        headers.insert(
            axum::http::header::ORIGIN,
            HeaderValue::from_static("http://127.0.0.1:6971"),
        );
        assert!(same_origin(&headers));
        headers.insert(
            axum::http::header::ORIGIN,
            HeaderValue::from_static("https://attacker.invalid"),
        );
        assert!(!same_origin(&headers));
    }

    #[test]
    fn cross_site_fetch_metadata_fails_even_without_origin() {
        let mut headers = HeaderMap::new();
        headers.insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
        assert!(!same_origin(&headers));
        headers.remove("sec-fetch-site");
        assert!(same_origin(&headers));
    }
}
