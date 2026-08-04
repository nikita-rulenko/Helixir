//! Graph-derived principal registry and group membership operations.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::rbac::{RbacManager, Role};
use super::rbac_compat::ONBOARDING_GROUP_ID;

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
}

impl RbacManager {
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

    /// Users that have ever crossed the onboarding admission boundary.
    ///
    /// Historical assignments count deliberately: removing a user from
    /// `onboarding` is an auditable offboarding event, not evidence that the
    /// compatibility migration missed that user.
    pub(crate) async fn onboarding_registered_user_ids(&self) -> Result<BTreeSet<String>> {
        let assignments: AssignmentsResponse = self
            .db
            .execute_query("getAllRbacAssignments", &serde_json::json!({}))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(assignments
            .assignments
            .into_iter()
            .filter(|assignment| {
                assignment.group_id == ONBOARDING_GROUP_ID
                    && Role::parse(&assignment.role).is_some()
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

        let agents = agents
            .agents
            .into_iter()
            .map(|agent| (agent.agent_id.clone(), agent))
            .collect::<BTreeMap<_, _>>();
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
            .users
            .into_iter()
            .filter(|user| !user.user_id.is_empty())
            .filter_map(|user| {
                let mut history = roles.remove(&user.user_id).unwrap_or_default();
                history.sort_by(|left, right| {
                    left.group_id
                        .cmp(&right.group_id)
                        .then(left.role.cmp(&right.role))
                        .then(left.created_at.cmp(&right.created_at))
                });
                let registered = history
                    .iter()
                    .any(|role| role.group_id.as_deref() == Some(ONBOARDING_GROUP_ID));
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
                let agent = agents.get(&user.user_id);
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
        if group_id != ONBOARDING_GROUP_ID {
            let policy = self.snapshot().await?;
            let enrolled = policy
                .users
                .get(user_id)
                .and_then(|binding| binding.groups.get(ONBOARDING_GROUP_ID))
                .is_some_and(|roles| !roles.is_empty());
            if !enrolled {
                anyhow::bail!(
                    "user '{user_id}' must be enrolled in '{ONBOARDING_GROUP_ID}' before joining '{group_id}'"
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
        if policy.enabled && !policy.is_admin(actor) {
            anyhow::bail!("RBAC group membership management requires a global admin");
        }
        let roles = policy
            .users
            .get(user_id)
            .and_then(|binding| binding.groups.get(group_id))
            .cloned()
            .unwrap_or_else(BTreeSet::new);
        let mut revoked = Vec::new();
        for role in roles {
            self.revoke_as(user_id, role, Some(group_id), actor).await?;
            revoked.push(role.label().to_string());
        }
        Ok(revoked)
    }
}

#[derive(Debug, Default, Deserialize)]
struct UsersResponse {
    #[serde(default)]
    users: Vec<UserNode>,
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

#[derive(Debug, Default, Deserialize)]
struct AgentsResponse {
    #[serde(default)]
    agents: Vec<AgentNode>,
}

#[derive(Debug, Deserialize)]
struct AgentNode {
    #[serde(default)]
    agent_id: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    last_seen: String,
}
