//! Graph-derived principal registry and group membership operations.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::rbac::{RbacManager, RbacMigrationState, Role, rbac_assignment_id};
use super::rbac_compat::{DEFAULT_GROUP_ID, ONBOARDING_GROUP_ID};
use super::rbac_registry_presence::{
    AgentsResponse, PrincipalPresence, resolve_presence_principal,
};

/// Result of the deliberately narrow remote-client admission operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientEnrollment {
    pub principal_id: String,
    pub group_id: String,
    pub roles: Vec<String>,
    pub created: bool,
}

/// One active or historical role assignment in the principal registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RbacRoleRecord {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    pub active: bool,
    pub granted_by: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub revoked_at: String,
}

/// Stable CLI/UI projection of one graph-backed user and optional agent presence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrincipalRecord {
    pub user_id: String,
    pub name: String,
    pub enrolled: bool,
    pub active_roles: Vec<RbacRoleRecord>,
    pub role_history: Vec<RbacRoleRecord>,
    pub agent_present: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent_status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent_last_seen: String,
    pub agent_instances: usize,
    pub subagents: usize,
}

impl RbacManager {
    /// Admit a remote client into the reserved onboarding workspace.
    ///
    /// This is the sole self-service RBAC mutation: the caller can name only
    /// itself, receives only `worker` in `onboarding`, and cannot select a
    /// group or role. Existing reserved-workspace grants are returned without
    /// mutation, so reconnecting never removes or downgrades access.
    pub async fn self_enroll_client(&self, principal_id: &str) -> Result<ClientEnrollment> {
        let principal_id = validate_client_principal(principal_id)?;
        let policy = self.snapshot().await?;
        if !policy.enabled || policy.migration_state != RbacMigrationState::Active {
            anyhow::bail!("RBAC onboarding is not active on this Helixir node");
        }
        if !policy.groups.contains_key(ONBOARDING_GROUP_ID) {
            anyhow::bail!("reserved onboarding workspace is missing");
        }

        let assignments: AssignmentsResponse = self
            .db
            .execute_query("getAllRbacAssignments", &serde_json::json!({}))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let historical_admission = assignments.assignments.iter().find(|assignment| {
            assignment.subject_id == principal_id
                && matches!(
                    assignment.group_id.as_str(),
                    ONBOARDING_GROUP_ID | DEFAULT_GROUP_ID
                )
                && Role::parse(&assignment.role).is_some()
        });
        if let Some(admission) = historical_admission {
            let active = policy.users.get(principal_id);
            let active_group = active.and_then(|binding| {
                [ONBOARDING_GROUP_ID, DEFAULT_GROUP_ID]
                    .into_iter()
                    .chain(binding.groups.keys().map(String::as_str))
                    .find_map(|group_id| {
                        binding
                            .groups
                            .get(group_id)
                            .filter(|roles| !roles.is_empty())
                            .map(|roles| (group_id, roles))
                    })
            });
            let (group_id, roles) = if let Some((group_id, roles)) = active_group {
                (
                    group_id.to_string(),
                    roles.iter().map(|role| role.label().to_string()).collect(),
                )
            } else if let Some(binding) = active.filter(|binding| !binding.global_roles.is_empty())
            {
                (
                    String::new(),
                    binding
                        .global_roles
                        .iter()
                        .map(|role| role.label().to_string())
                        .collect(),
                )
            } else {
                (admission.group_id.clone(), Vec::new())
            };
            return Ok(ClientEnrollment {
                principal_id: principal_id.to_string(),
                group_id,
                roles,
                created: false,
            });
        }

        let user_exists = match self
            .db
            .execute_query::<UserLookupResponse, _>(
                "getUser",
                &serde_json::json!({"user_id": principal_id}),
            )
            .await
        {
            Ok(response) => response.user.is_some(),
            // HelixDB v2.3.5 represents an empty `::FIRST` traversal as a
            // graph error rather than `{ user: null }`. For enrollment that
            // is the expected "new principal" branch, not a failed query.
            Err(error) if is_missing_user_lookup(&error.to_string()) => false,
            Err(error) => return Err(anyhow::anyhow!(error.to_string())),
        };
        if !user_exists {
            self.db
                .execute_query::<serde_json::Value, _>(
                    "ensureUser",
                    &serde_json::json!({"user_id": principal_id, "name": principal_id}),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }

        let role = Role::Worker;
        let created_at = chrono::Utc::now().to_rfc3339();
        self.db
            .execute_query::<serde_json::Value, _>(
                "grantRbacRole",
                &serde_json::json!({
                    "assignment_id": rbac_assignment_id(principal_id, ONBOARDING_GROUP_ID, role),
                    "subject_id": principal_id,
                    "role": role.label(),
                    "group_id": ONBOARDING_GROUP_ID,
                    "granted_by": principal_id,
                    "created_at": created_at,
                    "metadata": "{\"source\":\"remote-client-self-enrollment\"}",
                }),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;

        Ok(ClientEnrollment {
            principal_id: principal_id.to_string(),
            group_id: ONBOARDING_GROUP_ID.to_string(),
            roles: vec![role.label().to_string()],
            created: true,
        })
    }

    pub(crate) async fn all_user_ids(&self) -> Result<Vec<String>> {
        let users: UsersResponse = self
            .db
            .execute_query("getAllUsers", &serde_json::json!({}))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(users
            .users
            .into_iter()
            .filter_map(|user| (!user.user_id.is_empty()).then_some(user.user_id))
            .collect())
    }

    pub(crate) async fn reserved_registered_user_ids(&self) -> Result<BTreeSet<String>> {
        let assignments: AssignmentsResponse = self
            .db
            .execute_query("getAllRbacAssignments", &serde_json::json!({}))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(assignments
            .assignments
            .into_iter()
            .filter(|assignment| {
                matches!(
                    assignment.group_id.as_str(),
                    DEFAULT_GROUP_ID | ONBOARDING_GROUP_ID
                ) && Role::parse(&assignment.role).is_some()
                    && !assignment.subject_id.is_empty()
            })
            .map(|assignment| assignment.subject_id)
            .collect())
    }

    /// List registered users from HelixDB with active roles, audit history, and
    /// matching agent-presence state. No local registry is consulted.
    pub async fn principal_registry(&self, actor: &str) -> Result<Vec<PrincipalRecord>> {
        let policy = self.snapshot().await?;
        if policy.enabled && !policy.is_admin(actor) {
            anyhow::bail!("RBAC user registry inspection requires a global admin");
        }
        let users: UsersResponse = self
            .db
            .execute_query("getAllUsers", &serde_json::json!({}))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let assignments: AssignmentsResponse = self
            .db
            .execute_query("getAllRbacAssignments", &serde_json::json!({}))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let agents: AgentsResponse = self
            .db
            .execute_query("listAgents", &serde_json::json!({}))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;

        let users = users
            .users
            .into_iter()
            .filter(|user| !user.user_id.is_empty())
            .collect::<Vec<_>>();
        let known_principals = users
            .iter()
            .map(|user| user.user_id.clone())
            .collect::<BTreeSet<_>>();
        let mut agents_by_principal = BTreeMap::<String, PrincipalPresence>::new();
        for agent in agents.agents {
            if agent.agent_id.is_empty() {
                continue;
            }
            let principal_id =
                resolve_presence_principal(&agent.principal_id, &agent.agent_id, &known_principals);
            let is_subagent = agent.agent_id != principal_id;
            let aggregate = agents_by_principal.entry(principal_id).or_default();
            aggregate.instances += 1;
            aggregate.subagents += usize::from(is_subagent);
            if aggregate.last_seen <= agent.last_seen {
                aggregate.status = agent.status;
                aggregate.last_seen = agent.last_seen;
            }
        }
        let mut roles = BTreeMap::<String, Vec<RbacRoleRecord>>::new();
        for assignment in assignments.assignments {
            if Role::parse(&assignment.role).is_none() || assignment.subject_id.is_empty() {
                continue;
            }
            roles
                .entry(assignment.subject_id)
                .or_default()
                .push(RbacRoleRecord {
                    role: assignment.role,
                    group_id: (!assignment.group_id.is_empty()).then_some(assignment.group_id),
                    active: assignment.active != 0,
                    granted_by: assignment.granted_by,
                    created_at: assignment.created_at,
                    revoked_at: assignment.revoked_at,
                });
        }

        let mut registry = users
            .into_iter()
            .filter_map(|user| {
                let mut history = roles.remove(&user.user_id).unwrap_or_default();
                history.sort_by(|left, right| {
                    left.group_id
                        .cmp(&right.group_id)
                        .then(left.role.cmp(&right.role))
                        .then(left.created_at.cmp(&right.created_at))
                });
                let registered = history.iter().any(|role| {
                    matches!(
                        role.group_id.as_deref(),
                        Some(DEFAULT_GROUP_ID | ONBOARDING_GROUP_ID)
                    )
                });
                if !registered {
                    return None;
                }
                let active_roles = history
                    .iter()
                    .filter(|role| role.active)
                    .cloned()
                    .collect::<Vec<_>>();
                let enrolled = active_roles
                    .iter()
                    .any(|role| role.group_id.as_deref() == Some(ONBOARDING_GROUP_ID));
                let agent = agents_by_principal.get(&user.user_id);
                Some(PrincipalRecord {
                    user_id: user.user_id,
                    name: user.name,
                    enrolled,
                    active_roles,
                    role_history: history,
                    agent_present: agent.is_some(),
                    agent_status: agent.map(|agent| agent.status.clone()).unwrap_or_default(),
                    agent_last_seen: agent
                        .map(|agent| agent.last_seen.clone())
                        .unwrap_or_default(),
                    agent_instances: agent.map(|agent| agent.instances).unwrap_or_default(),
                    subagents: agent.map(|agent| agent.subagents).unwrap_or_default(),
                })
            })
            .collect::<Vec<_>>();
        registry.sort_by(|left, right| left.user_id.cmp(&right.user_id));
        Ok(registry)
    }

    /// Add a registered principal to a group. The reserved onboarding group is
    /// the only entry point for a new principal; every other group requires an
    /// active onboarding membership first.
    pub async fn add_user_to_group(
        &self,
        user_id: &str,
        group_id: &str,
        role: Role,
        actor: &str,
    ) -> Result<()> {
        if role == Role::Admin {
            anyhow::bail!("global admin cannot be group-scoped; use `rbac grant`");
        }
        if group_id == crate::core::rbac_compat::MOIRAI_GROUP_ID {
            anyhow::bail!("the reserved Moirai workspace accepts no role assignments");
        }
        if group_id != ONBOARDING_GROUP_ID {
            let registered = self.reserved_registered_user_ids().await?;
            if !registered.contains(user_id) {
                anyhow::bail!(
                    "user '{user_id}' must be registered through '{ONBOARDING_GROUP_ID}' before joining '{group_id}'"
                );
            }
        }
        self.grant(user_id, role, Some(group_id), actor).await
    }

    /// Revoke every active role a user holds in one group while retaining the
    /// User node and assignment history.
    pub async fn remove_user_from_group(
        &self,
        user_id: &str,
        group_id: &str,
        actor: &str,
    ) -> Result<Vec<String>> {
        let policy = self.snapshot().await?;
        self.authorize_group_management(actor, group_id).await?;
        let mut roles = policy
            .users
            .get(user_id)
            .and_then(|binding| binding.groups.get(group_id))
            .cloned()
            .unwrap_or_else(BTreeSet::new)
            .into_iter()
            .collect::<Vec<_>>();
        // A groupadmin removing itself must retain control until every other
        // role in this operation has been deactivated.
        roles.sort_by_key(|role| *role == Role::GroupAdmin);
        let mut revoked = Vec::new();
        for role in roles {
            self.revoke_as(user_id, role, Some(group_id), actor).await?;
            revoked.push(role.label().to_string());
        }
        Ok(revoked)
    }
}

fn validate_client_principal(value: &str) -> Result<&str> {
    let value = value.trim();
    if value.is_empty() || value.len() > 128 {
        anyhow::bail!("principal id must contain 1..=128 characters");
    }
    if !value.chars().all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || "-_.@".contains(character)
    }) {
        anyhow::bail!(
            "principal id may contain only lower-case ASCII letters, digits, '-', '_', '.', and '@'"
        );
    }
    Ok(value)
}

fn is_missing_user_lookup(error: &str) -> bool {
    error.to_ascii_lowercase().contains("no value found")
}

#[derive(Debug, Default, Deserialize)]
struct UsersResponse {
    #[serde(default)]
    users: Vec<UserNode>,
}

#[derive(Debug, Default, Deserialize)]
struct UserLookupResponse {
    #[serde(default)]
    user: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct UserNode {
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    name: String,
}

#[derive(Debug, Default, Deserialize)]
struct AssignmentsResponse {
    #[serde(default)]
    assignments: Vec<AssignmentNode>,
}

#[derive(Debug, Deserialize)]
struct AssignmentNode {
    #[serde(default)]
    subject_id: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    group_id: String,
    #[serde(default)]
    granted_by: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    revoked_at: String,
    #[serde(default)]
    active: i64,
}

#[cfg(test)]
mod tests {
    use super::{is_missing_user_lookup, validate_client_principal};

    #[test]
    fn client_principal_validation_is_stable_and_path_safe() {
        for accepted in ["codex", "codex-laptop", "agent_2", "nikita@workstation"] {
            assert_eq!(validate_client_principal(accepted).unwrap(), accepted);
        }
        for rejected in [
            "",
            "Codex",
            "two words",
            "../escape",
            "agent/token",
            "кириллица",
        ] {
            assert!(
                validate_client_principal(rejected).is_err(),
                "accepted {rejected}"
            );
        }
        assert!(validate_client_principal(&"a".repeat(129)).is_err());
    }

    #[test]
    fn missing_user_lookup_is_the_only_expected_empty_first_error() {
        assert!(is_missing_user_lookup("Graph error: No value found"));
        assert!(is_missing_user_lookup("GRAPH ERROR: NO VALUE FOUND"));
        assert!(!is_missing_user_lookup("connection refused"));
        assert!(!is_missing_user_lookup("schema mismatch"));
    }
}
