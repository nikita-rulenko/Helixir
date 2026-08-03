//! Role-based access control for Helixir memory operations.
//!
//! RBAC is deliberately opt-in.  A missing `RbacConfig` row (or `enabled = 0`)
//! preserves Helixir's historical full-trust deployment model.  When enabled,
//! the policy maps users to global roles and group-scoped roles.  Memory rows
//! remain owned by their existing `user_id`; group visibility is derived from
//! the owner's memberships, so authorship is not overloaded.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use anyhow::{Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::db::HelixClient;

/// A role understood by the RBAC policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Global administrator: unrestricted read/write access.
    Admin,
    /// Team lead: read access to explicitly assigned groups.
    TeamLead,
    /// Group administrator: unrestricted access inside assigned groups.
    GroupAdmin,
    /// Group moderator: read/write access to assigned groups.
    Moderator,
    /// Worker (employee or agent): read/write own authored memories in group.
    Worker,
    /// Viewer: read-only access to assigned groups.
    Viewer,
}

impl Role {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "admin" | "administrator" => Some(Self::Admin),
            "teamlead" | "team-lead" | "team_lead" | "lead" => Some(Self::TeamLead),
            "groupadmin" | "group-admin" | "group_admin" => Some(Self::GroupAdmin),
            "moderator" | "mod" => Some(Self::Moderator),
            "worker" | "member" => Some(Self::Worker),
            "viewer" | "read-only" | "readonly" => Some(Self::Viewer),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::TeamLead => "teamlead",
            Self::GroupAdmin => "groupadmin",
            Self::Moderator => "moderator",
            Self::Worker => "worker",
            Self::Viewer => "viewer",
        }
    }

    fn can_write(self) -> bool {
        !matches!(self, Self::Viewer | Self::TeamLead)
    }

    fn can_read(self) -> bool {
        true
    }
}

/// A named group.  The identifier is stable and is used by CLI scripts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// Roles assigned to one user.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserBinding {
    #[serde(default)]
    pub global_roles: BTreeSet<Role>,
    #[serde(default)]
    pub groups: BTreeMap<String, BTreeSet<Role>>,
}

/// Persisted RBAC document.  Keep this format boring and hand-editable: it is
/// also the audit/debug surface used by `helixir rbac export`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RbacPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub groups: BTreeMap<String, Group>,
    #[serde(default)]
    pub users: BTreeMap<String, UserBinding>,
}

impl RbacPolicy {
    pub fn group(&self, group: &str) -> Result<&Group> {
        self.groups
            .get(group)
            .ok_or_else(|| anyhow::anyhow!("unknown RBAC group '{group}'"))
    }

    pub fn upsert_group(&mut self, id: impl Into<String>, description: impl Into<String>) {
        let id = id.into();
        self.groups.insert(
            id.clone(),
            Group {
                name: id,
                description: description.into(),
            },
        );
    }

    pub fn remove_group(&mut self, group: &str) -> bool {
        let removed = self.groups.remove(group).is_some();
        for binding in self.users.values_mut() {
            binding.groups.remove(group);
        }
        removed
    }

    pub fn assign_global(&mut self, user: &str, role: Role) {
        self.users
            .entry(user.to_string())
            .or_default()
            .global_roles
            .insert(role);
    }

    pub fn assign_group(&mut self, user: &str, group: &str, role: Role) -> Result<()> {
        self.group(group)?;
        self.users
            .entry(user.to_string())
            .or_default()
            .groups
            .entry(group.to_string())
            .or_default()
            .insert(role);
        Ok(())
    }

    pub fn revoke_global(&mut self, user: &str, role: Role) -> bool {
        self.users
            .get_mut(user)
            .is_some_and(|binding| binding.global_roles.remove(&role))
    }

    pub fn revoke_group(&mut self, user: &str, group: &str, role: Role) -> bool {
        let Some(binding) = self.users.get_mut(user) else {
            return false;
        };
        let Some(roles) = binding.groups.get_mut(group) else {
            return false;
        };
        let removed = roles.remove(&role);
        if roles.is_empty() {
            binding.groups.remove(group);
        }
        removed
    }

    pub fn roles_for(&self, user: &str) -> Vec<(Option<&str>, Role)> {
        let Some(binding) = self.users.get(user) else {
            return Vec::new();
        };
        let mut out = binding
            .global_roles
            .iter()
            .copied()
            .map(|role| (None, role))
            .collect::<Vec<_>>();
        for (group, roles) in &binding.groups {
            out.extend(
                roles
                    .iter()
                    .copied()
                    .map(|role| (Some(group.as_str()), role)),
            );
        }
        out
    }

    pub fn is_admin(&self, user: &str) -> bool {
        self.users
            .get(user)
            .is_some_and(|binding| binding.global_roles.contains(&Role::Admin))
    }

    /// Groups whose memories the actor can read.
    pub fn readable_groups(&self, actor: &str) -> Option<HashSet<String>> {
        if !self.enabled {
            return None;
        }
        let Some(binding) = self.users.get(actor) else {
            return Some(HashSet::new());
        };
        if binding.global_roles.contains(&Role::Admin) {
            return None;
        }
        Some(
            binding
                .groups
                .iter()
                .filter(|(_, roles)| roles.iter().any(|role| role.can_read()))
                .map(|(group, _)| group.clone())
                .collect(),
        )
    }

    /// Return the user ids whose memories are visible to `actor`.
    /// `None` means unrestricted (global admin); an empty set means deny.
    pub fn readable_users(&self, actor: &str) -> Option<HashSet<String>> {
        let groups = self.readable_groups(actor)?;
        let mut users = HashSet::new();
        if self
            .users
            .get(actor)
            .is_some_and(|binding| !binding.global_roles.is_empty() || !binding.groups.is_empty())
        {
            users.insert(actor.to_string());
        }
        for (user, binding) in &self.users {
            if binding.groups.keys().any(|group| groups.contains(group)) {
                users.insert(user.clone());
            }
        }
        Some(users)
    }

    pub fn can_write(&self, actor: &str) -> bool {
        if !self.enabled {
            return true;
        }
        let Some(binding) = self.users.get(actor) else {
            return false;
        };
        binding.global_roles.iter().any(|role| role.can_write())
            || binding
                .groups
                .values()
                .flatten()
                .any(|role| role.can_write())
    }

    pub fn can_write_owner(&self, actor: &str, owner: &str) -> bool {
        if !self.enabled {
            return true;
        }
        if self.is_admin(actor) {
            return true;
        }
        if actor == owner && self.can_write(actor) {
            return true;
        }
        let Some(actor_groups) = self.readable_groups(actor) else {
            return true;
        };
        let owner_groups = self
            .users
            .get(owner)
            .map(|binding| binding.groups.keys().collect::<HashSet<_>>())
            .unwrap_or_default();
        self.users.get(actor).is_some_and(|binding| {
            binding.groups.iter().any(|(group, roles)| {
                actor_groups.contains(group)
                    && owner_groups.contains(group)
                    && roles
                        .iter()
                        .any(|role| matches!(role, Role::GroupAdmin | Role::Moderator))
            })
        })
    }

    /// Whether an actor may create a new memory owned by `owner`.
    /// Global administrators can write for anyone; group administrators and
    /// moderators can write for members of their groups; workers can only
    /// create memories under their own identity.
    pub fn can_create_for(&self, actor: &str, owner: &str) -> bool {
        if !self.enabled {
            return true;
        }
        if actor == owner {
            return self.can_write(actor);
        }
        if self.is_admin(actor) {
            return true;
        }
        let Some(actor_binding) = self.users.get(actor) else {
            return false;
        };
        let Some(owner_binding) = self.users.get(owner) else {
            return false;
        };
        actor_binding.groups.iter().any(|(group, roles)| {
            owner_binding.groups.contains_key(group)
                && roles
                    .iter()
                    .any(|role| matches!(role, Role::GroupAdmin | Role::Moderator))
        })
    }

    /// Filter search rows using the stable `metadata.user_id` provenance field.
    pub fn filter_results<T>(
        &self,
        actor: &str,
        results: Vec<T>,
        owner: impl Fn(&T) -> Option<&str>,
    ) -> Vec<T> {
        let Some(allowed) = self.readable_users(actor) else {
            return results;
        };
        results
            .into_iter()
            .filter(|row| owner(row).is_some_and(|user| allowed.contains(user)))
            .collect()
    }

    pub fn validate(&self) -> Result<()> {
        for (user, binding) in &self.users {
            if user.trim().is_empty() {
                bail!("RBAC user id cannot be empty");
            }
            for group in binding.groups.keys() {
                self.group(group)?;
            }
        }
        Ok(())
    }
}

/// HelixDB-backed RBAC service.  The service is intentionally small: all
/// callers (CLI, MCP and the public client facade) resolve the same snapshot
/// through these named queries, so no host-local policy can diverge.
#[derive(Clone)]
pub struct RbacManager {
    db: Arc<HelixClient>,
}

impl RbacManager {
    pub fn new(db: Arc<HelixClient>) -> Self {
        Self { db }
    }

    async fn authorize_admin(&self, actor: &str) -> Result<()> {
        let policy = self.snapshot().await?;
        if policy.enabled && !policy.is_admin(actor) {
            bail!("RBAC management requires a global admin");
        }
        Ok(())
    }

    pub async fn snapshot(&self) -> Result<RbacPolicy> {
        let enabled = match self
            .db
            .execute_query::<serde_json::Value, _>("getRbacConfig", &serde_json::json!({}))
            .await
        {
            Ok(value) => {
                value
                    .get("config")
                    .and_then(|config| config.get("enabled"))
                    .and_then(number_as_i64)
                    .unwrap_or(0)
                    != 0
            }
            Err(error) if is_missing_rbac_surface(&error.to_string()) => false,
            Err(error) => return Err(anyhow::anyhow!(error.to_string())),
        };

        let groups_value: serde_json::Value = match self
            .db
            .execute_query("getRbacGroups", &serde_json::json!({}))
            .await
        {
            Ok(value) => value,
            Err(error) if !enabled && is_missing_rbac_surface(&error.to_string()) => {
                return Ok(RbacPolicy::default());
            }
            Err(error) => return Err(anyhow::anyhow!(error.to_string())),
        };
        let assignments_value: serde_json::Value = match self
            .db
            .execute_query("getRbacAssignments", &serde_json::json!({}))
            .await
        {
            Ok(value) => value,
            Err(error) if !enabled && is_missing_rbac_surface(&error.to_string()) => {
                return Ok(RbacPolicy::default());
            }
            Err(error) => return Err(anyhow::anyhow!(error.to_string())),
        };

        let mut policy = RbacPolicy {
            enabled,
            ..Default::default()
        };
        for row in rows(&groups_value, "groups") {
            let Some(group_id) = string_field(row, "group_id") else {
                continue;
            };
            policy.groups.insert(
                group_id.clone(),
                Group {
                    name: string_field(row, "name").unwrap_or_else(|| group_id.clone()),
                    description: string_field(row, "description").unwrap_or_default(),
                },
            );
        }
        for row in rows(&assignments_value, "assignments") {
            let (Some(subject), Some(role_name)) =
                (string_field(row, "subject_id"), string_field(row, "role"))
            else {
                continue;
            };
            let Some(role) = Role::parse(&role_name) else {
                continue;
            };
            let group = string_field(row, "group_id").unwrap_or_default();
            if group.is_empty() {
                policy.assign_global(&subject, role);
            } else if policy.groups.contains_key(&group) {
                // The DB has already validated the group at grant time.  A
                // deleted group is ignored defensively rather than widening access.
                let _ = policy.assign_group(&subject, &group, role);
            }
        }
        policy.validate()?;
        Ok(policy)
    }

    pub async fn set_enabled(&self, enabled: bool, actor: &str) -> Result<()> {
        self.authorize_admin(actor).await?;
        self.db
            .execute_query::<serde_json::Value, _>(
                "setRbacEnabled",
                &serde_json::json!({
                    "enabled": i64::from(enabled),
                    "updated_at": Utc::now().to_rfc3339(),
                    "updated_by": actor,
                }),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(())
    }

    pub async fn create_group(&self, group_id: &str, name: &str, description: &str) -> Result<()> {
        self.create_group_as(group_id, name, description, "").await
    }

    pub async fn create_group_as(
        &self,
        group_id: &str,
        name: &str,
        description: &str,
        actor: &str,
    ) -> Result<()> {
        self.authorize_admin(actor).await?;
        self.db
            .execute_query::<serde_json::Value, _>(
                "createRbacGroup",
                &serde_json::json!({
                    "group_id": group_id,
                    "name": name,
                    "description": description,
                    "created_at": Utc::now().to_rfc3339(),
                    "metadata": "{}",
                }),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(())
    }

    pub async fn deactivate_group(&self, group_id: &str) -> Result<()> {
        self.deactivate_group_as(group_id, "").await
    }

    pub async fn deactivate_group_as(&self, group_id: &str, actor: &str) -> Result<()> {
        self.authorize_admin(actor).await?;
        self.db
            .execute_query::<serde_json::Value, _>(
                "deactivateRbacGroup",
                &serde_json::json!({"group_id": group_id}),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(())
    }

    pub async fn grant(
        &self,
        subject_id: &str,
        role: Role,
        group_id: Option<&str>,
        granted_by: &str,
    ) -> Result<()> {
        self.authorize_admin(granted_by).await?;
        let group = group_id.unwrap_or("");
        if !group.is_empty() {
            if matches!(role, Role::Admin) {
                bail!("global admin cannot be group-scoped; use groupadmin");
            }
            let policy = self.snapshot().await?;
            policy.group(group)?;
        } else if !matches!(role, Role::Admin) {
            bail!(
                "role '{}' requires --group; only admin is global",
                role.label()
            );
        }
        let user_exists = self
            .db
            .execute_query::<serde_json::Value, _>(
                "getUser",
                &serde_json::json!({"user_id": subject_id}),
            )
            .await
            .is_ok();
        if !user_exists {
            self.db
                .execute_query::<serde_json::Value, _>(
                    "addUser",
                    &serde_json::json!({"user_id": subject_id, "name": subject_id}),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        let query = if group.is_empty() {
            "grantRbacGlobalRole"
        } else {
            "grantRbacRole"
        };
        let mut params = serde_json::json!({
            "assignment_id": assignment_id(subject_id, group, role),
            "subject_id": subject_id,
            "role": role.label(),
            "group_id": group,
            "granted_by": granted_by,
            "created_at": Utc::now().to_rfc3339(),
            "metadata": "{}",
        });
        if group.is_empty() {
            if let Some(object) = params.as_object_mut() {
                object.remove("group_id");
            }
        }
        self.db
            .execute_query::<serde_json::Value, _>(query, &params)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(())
    }

    /// Attach a newly-authored memory to every active group of its author.
    /// This makes group membership an actual graph edge while retaining the
    /// existing `Memory.user_id` author field for provenance.
    pub async fn link_memory_to_actor_groups(&self, memory_id: &str, actor: &str) -> Result<()> {
        let policy = self.snapshot().await?;
        let Some(binding) = policy.users.get(actor) else {
            return Ok(());
        };
        for group in binding.groups.keys() {
            self.db
                .execute_query::<serde_json::Value, _>(
                    "linkMemoryToRbacGroup",
                    &serde_json::json!({
                        "memory_id": memory_id,
                        "group_id": group,
                        "assigned_by": actor,
                        "assigned_at": Utc::now().to_rfc3339(),
                    }),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        Ok(())
    }

    pub async fn revoke(&self, subject_id: &str, role: Role, group_id: Option<&str>) -> Result<()> {
        self.revoke_as(subject_id, role, group_id, "").await
    }

    pub async fn revoke_as(
        &self,
        subject_id: &str,
        role: Role,
        group_id: Option<&str>,
        revoked_by: &str,
    ) -> Result<()> {
        self.authorize_admin(revoked_by).await?;
        self.db
            .execute_query::<serde_json::Value, _>(
                "revokeRbacRole",
                &serde_json::json!({
                    "subject_id": subject_id,
                    "role": role.label(),
                    "group_id": group_id.unwrap_or(""),
                    "revoked_at": Utc::now().to_rfc3339(),
                }),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(())
    }

    pub async fn authorize_write(&self, actor: &str) -> Result<()> {
        let policy = self.snapshot().await?;
        if !policy.can_write(actor) {
            bail!("RBAC denied write for '{actor}'")
        }
        Ok(())
    }

    pub async fn authorize_write_for(&self, actor: &str, owner: &str) -> Result<()> {
        let policy = self.snapshot().await?;
        if !policy.can_create_for(actor, owner) {
            bail!("RBAC denied write for '{actor}' as owner '{owner}'")
        }
        Ok(())
    }

    pub async fn authorize_owner_write(&self, actor: &str, owner: &str) -> Result<()> {
        let policy = self.snapshot().await?;
        if !policy.can_write_owner(actor, owner) {
            bail!("RBAC denied write for '{actor}' on memory owned by '{owner}'")
        }
        Ok(())
    }
}

fn assignment_id(subject: &str, group: &str, role: Role) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(subject.as_bytes());
    hasher.update([0]);
    hasher.update(group.as_bytes());
    hasher.update([0]);
    hasher.update(role.label().as_bytes());
    format!("rbac_{:x}", hasher.finalize())
}

fn rows<'a>(value: &'a serde_json::Value, key: &str) -> Vec<&'a serde_json::Value> {
    value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .or_else(|| value.as_array())
        .map(|rows| rows.iter().collect())
        .unwrap_or_default()
}

fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|field| field.as_str())
        .map(str::to_owned)
}

fn number_as_i64(value: &serde_json::Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

fn is_missing_rbac_surface(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("couldn't find")
        || lower.contains("could not find")
        || lower.contains("no value found")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RbacPolicy {
        let mut p = RbacPolicy {
            enabled: true,
            ..Default::default()
        };
        p.upsert_group("alpha", "Alpha team");
        p.upsert_group("beta", "Beta team");
        p.assign_global("root", Role::Admin);
        p.assign_group("lead", "alpha", Role::TeamLead).unwrap();
        p.assign_group("mod", "alpha", Role::Moderator).unwrap();
        p.assign_group("worker", "alpha", Role::Worker).unwrap();
        p.assign_group("viewer", "alpha", Role::Viewer).unwrap();
        p
    }

    #[test]
    fn role_parser_accepts_cli_aliases() {
        assert_eq!(Role::parse("team-lead"), Some(Role::TeamLead));
        assert_eq!(Role::parse("read-only"), Some(Role::Viewer));
        assert_eq!(Role::parse("nope"), None);
    }

    #[test]
    fn admin_reads_everyone_and_viewer_only_group() {
        let p = sample();
        assert!(p.readable_users("root").is_none());
        let visible = p.readable_users("viewer").unwrap();
        assert!(visible.contains("worker"));
        assert!(visible.contains("viewer"));
        assert!(!visible.contains("root"));
    }

    #[test]
    fn viewer_cannot_write_but_worker_can_write_own_memory() {
        let p = sample();
        assert!(!p.can_write("viewer"));
        assert!(p.can_write_owner("worker", "worker"));
        assert!(p.can_write_owner("mod", "worker"));
        assert!(!p.can_write_owner("worker", "mod"));
    }

    #[test]
    fn disabled_policy_is_full_trust() {
        let p = RbacPolicy::default();
        assert!(p.can_write("unknown"));
        assert!(p.readable_users("unknown").is_none());
    }

    #[test]
    fn deny_by_default_for_unassigned_principal() {
        let p = sample();
        assert!(p.readable_users("unassigned").unwrap().is_empty());
        assert!(!p.can_write("unassigned"));
    }

    #[test]
    fn every_role_has_expected_write_semantics() {
        let mut p = RbacPolicy {
            enabled: true,
            ..Default::default()
        };
        p.upsert_group("g", "Group");
        for (user, role) in [
            ("admin", Role::Admin),
            ("lead", Role::TeamLead),
            ("group-admin", Role::GroupAdmin),
            ("moderator", Role::Moderator),
            ("worker", Role::Worker),
            ("viewer", Role::Viewer),
        ] {
            if role == Role::Admin {
                p.assign_global(user, role);
            } else {
                p.assign_group(user, "g", role).unwrap();
            }
        }
        assert!(p.can_write("admin"));
        assert!(!p.can_write("lead"));
        assert!(p.can_write("group-admin"));
        assert!(p.can_write("moderator"));
        assert!(p.can_write("worker"));
        assert!(!p.can_write("viewer"));
    }

    #[test]
    fn groups_isolate_readers() {
        let mut p = RbacPolicy {
            enabled: true,
            ..Default::default()
        };
        p.upsert_group("a", "A");
        p.upsert_group("b", "B");
        p.assign_group("alice", "a", Role::Worker).unwrap();
        p.assign_group("bob", "b", Role::Worker).unwrap();
        p.assign_group("auditor", "a", Role::Viewer).unwrap();
        let visible = p.readable_users("auditor").unwrap();
        assert!(visible.contains("alice"));
        assert!(!visible.contains("bob"));
    }

    #[test]
    fn actor_and_owner_are_checked_separately_for_new_memories() {
        let mut p = sample();
        p.assign_group("group-admin", "alpha", Role::GroupAdmin)
            .unwrap();
        assert!(p.can_create_for("root", "worker"));
        assert!(p.can_create_for("mod", "worker"));
        assert!(p.can_create_for("worker", "worker"));
        assert!(!p.can_create_for("worker", "mod"));
        assert!(!p.can_create_for("viewer", "worker"));
        assert!(p.can_create_for("group-admin", "worker"));
    }

    #[test]
    fn only_missing_schema_errors_preserve_legacy_disabled_mode() {
        assert!(is_missing_rbac_surface(
            "Couldn't find setRbacEnabled of type Query (NOT_FOUND)"
        ));
        assert!(is_missing_rbac_surface("Graph error: No value found"));
        assert!(!is_missing_rbac_surface("Connection failed: timeout"));
        assert!(!is_missing_rbac_surface("permission denied"));
    }
}
