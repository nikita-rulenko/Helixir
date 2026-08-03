//! Role-based access control for Helixir memory operations.
//!
//! RBAC is deliberately opt-in.  A missing `RbacConfig` row (or `enabled = 0`)
//! preserves Helixir's historical full-trust deployment model.  When enabled,
//! the policy maps users to global roles and group-scoped roles. Memory rows
//! remain owned by their existing `user_id`; strict visibility is derived from
//! explicit `MEMORY_IN_RBAC_GROUP` edges, so authorship is not overloaded and
//! a multi-group owner cannot accidentally share one memory with every group.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_group_id: Option<String>,
}

/// A stable federation of RBAC groups that intentionally shares deduplication
/// and visibility for memories created while those groups are members.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DedupGroup {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// Security domain resolved for one memory write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RbacMemoryScope {
    /// Historical full-trust behavior: global dedup and no RBAC edges.
    Legacy,
    /// Enabled-mode global-admin write with no group visibility.
    Unscoped,
    /// Private deduplication and visibility inside one concrete group.
    Group { group_id: String },
    /// Federated deduplication with materialized visibility for the current
    /// member groups.
    DedupGroup {
        dedup_group_id: String,
        group_ids: BTreeSet<String>,
    },
}

impl RbacMemoryScope {
    /// Stable salt for the content fingerprint. `None` preserves byte-for-byte
    /// legacy keys while every enabled RBAC domain gets an isolated namespace.
    pub fn fingerprint_scope(&self) -> Option<String> {
        match self {
            Self::Legacy => None,
            Self::Unscoped => Some("rbac:unscoped".to_string()),
            Self::Group { group_id } => Some(format!("rbac:group:{group_id}")),
            Self::DedupGroup { dedup_group_id, .. } => Some(format!("rbac:dedup:{dedup_group_id}")),
        }
    }

    pub fn group_ids(&self) -> BTreeSet<String> {
        match self {
            Self::Group { group_id } => BTreeSet::from([group_id.clone()]),
            Self::DedupGroup { group_ids, .. } => group_ids.clone(),
            Self::Legacy | Self::Unscoped => BTreeSet::new(),
        }
    }
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
    pub dedup_groups: BTreeMap<String, DedupGroup>,
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
                dedup_group_id: None,
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

    pub fn upsert_dedup_group(&mut self, id: impl Into<String>, description: impl Into<String>) {
        let id = id.into();
        self.dedup_groups.insert(
            id.clone(),
            DedupGroup {
                name: id,
                description: description.into(),
            },
        );
    }

    pub fn assign_dedup_group(&mut self, group: &str, dedup_group: Option<&str>) -> Result<()> {
        if let Some(id) = dedup_group
            && !self.dedup_groups.contains_key(id)
        {
            bail!("unknown RBAC dedup group '{id}'");
        }
        self.group(group)?;
        if let Some(entry) = self.groups.get_mut(group) {
            entry.dedup_group_id = dedup_group.map(str::to_string);
        }
        Ok(())
    }

    pub fn resolve_memory_scope(&self, group_id: Option<&str>) -> Result<RbacMemoryScope> {
        if !self.enabled {
            return Ok(RbacMemoryScope::Legacy);
        }
        let Some(group_id) = group_id else {
            return Ok(RbacMemoryScope::Unscoped);
        };
        let group = self.group(group_id)?;
        let Some(dedup_group_id) = group.dedup_group_id.as_deref() else {
            return Ok(RbacMemoryScope::Group {
                group_id: group_id.to_string(),
            });
        };
        let group_ids = self
            .groups
            .iter()
            .filter(|(_, group)| group.dedup_group_id.as_deref() == Some(dedup_group_id))
            .map(|(id, _)| id.clone())
            .collect();
        Ok(RbacMemoryScope::DedupGroup {
            dedup_group_id: dedup_group_id.to_string(),
            group_ids,
        })
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

    /// Whether `actor` may create a memory owned by `owner` in one explicit
    /// group. Enabled non-admin writes never infer a group from the owner's
    /// memberships: the caller must select it.
    pub fn can_create_for_group(&self, actor: &str, owner: &str, group: Option<&str>) -> bool {
        if !self.enabled {
            return true;
        }
        if self.is_admin(actor) {
            return group.is_none() || group.is_some_and(|id| self.groups.contains_key(id));
        }
        let Some(group) = group else {
            return false;
        };
        if !self.groups.contains_key(group) {
            return false;
        }
        let Some(actor_binding) = self.users.get(actor) else {
            return false;
        };
        let Some(actor_roles) = actor_binding.groups.get(group) else {
            return false;
        };
        if actor == owner {
            return actor_roles.iter().any(|role| role.can_write());
        }
        let owner_is_member = self
            .users
            .get(owner)
            .is_some_and(|binding| binding.groups.contains_key(group));
        owner_is_member
            && actor_roles
                .iter()
                .any(|role| matches!(role, Role::GroupAdmin | Role::Moderator))
    }

    /// Whether `actor` may mutate an existing memory with the supplied group
    /// edges. An unscoped memory is admin-only while RBAC is enabled.
    pub fn can_write_memory(
        &self,
        actor: &str,
        owner: &str,
        memory_groups: &HashSet<String>,
    ) -> bool {
        if !self.enabled || self.is_admin(actor) {
            return true;
        }
        let Some(actor_binding) = self.users.get(actor) else {
            return false;
        };
        if actor == owner {
            return memory_groups.iter().any(|group| {
                actor_binding
                    .groups
                    .get(group)
                    .is_some_and(|roles| roles.iter().any(|role| role.can_write()))
            });
        }
        let Some(owner_binding) = self.users.get(owner) else {
            return false;
        };
        memory_groups.iter().any(|group| {
            owner_binding.groups.contains_key(group)
                && actor_binding.groups.get(group).is_some_and(|roles| {
                    roles
                        .iter()
                        .any(|role| matches!(role, Role::GroupAdmin | Role::Moderator))
                })
        })
    }

    /// Legacy owner-level filtering used only by disabled-mode compatibility
    /// callers. Enabled read paths must use `RbacManager::visible_memory_ids`
    /// so authorization is based on per-memory group edges.
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
        for group in self.groups.values() {
            if let Some(dedup_group) = group.dedup_group_id.as_deref()
                && !self.dedup_groups.contains_key(dedup_group)
            {
                bail!("unknown RBAC dedup group '{dedup_group}'");
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
        let dedup_groups_value: serde_json::Value = match self
            .db
            .execute_query("getRbacDedupGroups", &serde_json::json!({}))
            .await
        {
            Ok(value) => value,
            Err(error) if !enabled && is_missing_rbac_surface(&error.to_string()) => {
                serde_json::json!({"dedup_groups": []})
            }
            Err(error) => return Err(anyhow::anyhow!(error.to_string())),
        };
        let dedup_memberships_value: serde_json::Value = match self
            .db
            .execute_query("getRbacDedupMemberships", &serde_json::json!({}))
            .await
        {
            Ok(value) => value,
            Err(error) if !enabled && is_missing_rbac_surface(&error.to_string()) => {
                serde_json::json!({"groups": [], "links": [], "dedup_groups": []})
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
                    dedup_group_id: None,
                },
            );
        }
        for row in rows(&dedup_groups_value, "dedup_groups") {
            let Some(dedup_group_id) = string_field(row, "dedup_group_id") else {
                continue;
            };
            policy.dedup_groups.insert(
                dedup_group_id.clone(),
                DedupGroup {
                    name: string_field(row, "name").unwrap_or_else(|| dedup_group_id.clone()),
                    description: string_field(row, "description").unwrap_or_default(),
                },
            );
        }
        apply_dedup_memberships(&mut policy, &dedup_memberships_value);
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

    pub async fn create_dedup_group_as(
        &self,
        dedup_group_id: &str,
        name: &str,
        description: &str,
        actor: &str,
    ) -> Result<()> {
        self.authorize_admin(actor).await?;
        self.db
            .execute_query::<serde_json::Value, _>(
                "createRbacDedupGroup",
                &serde_json::json!({
                    "dedup_group_id": dedup_group_id,
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

    pub async fn deactivate_dedup_group_as(&self, dedup_group_id: &str, actor: &str) -> Result<()> {
        self.authorize_admin(actor).await?;
        let policy = self.snapshot().await?;
        if policy
            .groups
            .values()
            .any(|group| group.dedup_group_id.as_deref() == Some(dedup_group_id))
        {
            bail!("dedup group '{dedup_group_id}' still has active member groups")
        }
        self.db
            .execute_query::<serde_json::Value, _>(
                "deactivateRbacDedupGroup",
                &serde_json::json!({"dedup_group_id": dedup_group_id}),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(())
    }

    /// Join a group to a dedup federation and materialize access to the
    /// federation's existing memories. Retrying is safe and completes any
    /// interrupted backfill.
    pub async fn attach_group_to_dedup_as(
        &self,
        group_id: &str,
        dedup_group_id: &str,
        actor: &str,
    ) -> Result<usize> {
        self.authorize_admin(actor).await?;
        let policy = self.snapshot().await?;
        policy.group(group_id)?;
        if !policy.dedup_groups.contains_key(dedup_group_id) {
            bail!("unknown RBAC dedup group '{dedup_group_id}'")
        }
        if let Some(current) = policy
            .groups
            .get(group_id)
            .and_then(|group| group.dedup_group_id.as_deref())
            && current != dedup_group_id
        {
            self.db
                .execute_query::<serde_json::Value, _>(
                    "clearRbacGroupDedupMembership",
                    &serde_json::json!({
                        "group_id": group_id,
                        "removed_at": Utc::now().to_rfc3339(),
                    }),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        self.db
            .execute_query::<serde_json::Value, _>(
                "setRbacGroupDedupMembership",
                &serde_json::json!({
                    "group_id": group_id,
                    "dedup_group_id": dedup_group_id,
                    "assigned_by": actor,
                    "assigned_at": Utc::now().to_rfc3339(),
                }),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;

        let response: DedupGroupMemoriesResponse = self
            .db
            .execute_query(
                "getMemoriesInRbacDedupGroup",
                &serde_json::json!({"dedup_group_id": dedup_group_id}),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let mut linked = 0usize;
        for memory in response.memories {
            if memory.memory_id.is_empty() {
                continue;
            }
            self.link_memory_to_group(&memory.memory_id, Some(group_id), actor)
                .await?;
            linked += 1;
        }
        Ok(linked)
    }

    /// Leave a dedup federation prospectively. Historical memory-to-group
    /// edges are intentionally retained.
    pub async fn detach_group_from_dedup_as(&self, group_id: &str, actor: &str) -> Result<()> {
        self.authorize_admin(actor).await?;
        let policy = self.snapshot().await?;
        let group = policy.group(group_id)?;
        if group.dedup_group_id.is_none() {
            return Ok(());
        }
        self.db
            .execute_query::<serde_json::Value, _>(
                "clearRbacGroupDedupMembership",
                &serde_json::json!({
                    "group_id": group_id,
                    "removed_at": Utc::now().to_rfc3339(),
                }),
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

    /// Attach a memory to one explicit group. `UpsertE` makes retries and Hive
    /// dedup links idempotent for the same `(memory, group)` pair.
    pub async fn link_memory_to_group(
        &self,
        memory_id: &str,
        group_id: Option<&str>,
        actor: &str,
    ) -> Result<()> {
        let Some(group_id) = group_id else {
            return Ok(());
        };
        self.db
            .execute_query::<serde_json::Value, _>(
                "linkMemoryToRbacGroup",
                &serde_json::json!({
                    "memory_id": memory_id,
                    "group_id": group_id,
                    "assigned_by": actor,
                    "assigned_at": Utc::now().to_rfc3339(),
                }),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(())
    }

    pub async fn resolve_write_scope(&self, group_id: Option<&str>) -> Result<RbacMemoryScope> {
        self.snapshot().await?.resolve_memory_scope(group_id)
    }

    /// Materialize the access and dedup provenance edges for a completed
    /// memory operation. Federation writes link every current member group;
    /// those edges remain historical when membership later changes.
    pub async fn link_memory_to_scope(
        &self,
        memory_id: &str,
        scope: &RbacMemoryScope,
        actor: &str,
    ) -> Result<()> {
        if let RbacMemoryScope::DedupGroup { dedup_group_id, .. } = scope {
            self.db
                .execute_query::<serde_json::Value, _>(
                    "linkMemoryToRbacDedupGroup",
                    &serde_json::json!({
                        "memory_id": memory_id,
                        "dedup_group_id": dedup_group_id,
                        "assigned_by": actor,
                        "assigned_at": Utc::now().to_rfc3339(),
                    }),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        for group_id in scope.group_ids() {
            self.link_memory_to_group(memory_id, Some(&group_id), actor)
                .await?;
        }
        Ok(())
    }

    /// Return candidate ids belonging to exactly the requested dedup domain.
    /// `None` means legacy full-trust and therefore no filtering.
    pub async fn memory_ids_in_scope(
        &self,
        scope: &RbacMemoryScope,
        memory_ids: &[String],
    ) -> Result<Option<HashSet<String>>> {
        if matches!(scope, RbacMemoryScope::Legacy) {
            return Ok(None);
        }
        if memory_ids.is_empty() {
            return Ok(Some(HashSet::new()));
        }
        let scope_map = self.memory_scope_map(memory_ids).await?;
        let expected_scope = scope.fingerprint_scope().unwrap_or_default();
        Ok(Some(
            memory_ids
                .iter()
                .filter(|memory_id| {
                    let stored = scope_map
                        .get(memory_id.as_str())
                        .cloned()
                        .unwrap_or_default();
                    stored.rbac_scope == expected_scope
                })
                .cloned()
                .collect(),
        ))
    }

    /// Return the subset of `memory_ids` visible to `actor`. `None` means the
    /// actor is unrestricted (RBAC disabled or global admin). Enabled
    /// non-admin reads fail closed for unscoped memories.
    pub async fn visible_memory_ids(
        &self,
        actor: &str,
        memory_ids: &[String],
    ) -> Result<Option<HashSet<String>>> {
        let policy = self.snapshot().await?;
        let Some(readable_groups) = policy.readable_groups(actor) else {
            return Ok(None);
        };
        if readable_groups.is_empty() || memory_ids.is_empty() {
            return Ok(Some(HashSet::new()));
        }
        let group_map = self.memory_group_map(memory_ids).await?;
        Ok(Some(
            memory_ids
                .iter()
                .filter(|memory_id| {
                    group_map
                        .get(memory_id.as_str())
                        .is_some_and(|groups| !groups.is_disjoint(&readable_groups))
                })
                .cloned()
                .collect(),
        ))
    }

    /// Resolve explicit group edges for a batch of memory ids.
    pub async fn memory_group_map(
        &self,
        memory_ids: &[String],
    ) -> Result<HashMap<String, HashSet<String>>> {
        Ok(self
            .memory_scope_map(memory_ids)
            .await?
            .into_iter()
            .map(|(memory_id, scope)| (memory_id, scope.groups))
            .collect())
    }

    async fn memory_scope_map(
        &self,
        memory_ids: &[String],
    ) -> Result<HashMap<String, StoredMemoryScope>> {
        if memory_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let response: MemoryRbacScopesResponse = self
            .db
            .execute_query(
                "getMemoryRbacScopesBatch",
                &serde_json::json!({"memory_ids": memory_ids}),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;

        let memories = response
            .memories
            .into_iter()
            .map(|memory| {
                (
                    memory.id,
                    (memory.memory_id, memory.rbac_scope.unwrap_or_default()),
                )
            })
            .collect::<HashMap<_, _>>();
        let groups = response
            .groups
            .into_iter()
            .map(|group| (group.id, group.group_id))
            .collect::<HashMap<_, _>>();
        let dedup_groups = response
            .dedup_groups
            .into_iter()
            .map(|group| (group.id, group.dedup_group_id))
            .collect::<HashMap<_, _>>();
        let mut result = memories
            .values()
            .cloned()
            .map(|(memory_id, rbac_scope)| {
                (
                    memory_id,
                    StoredMemoryScope {
                        rbac_scope,
                        ..Default::default()
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        for link in response.group_links {
            let (Some(memory_id), Some(group_id)) =
                (memories.get(&link.from_node), groups.get(&link.to_node))
            else {
                continue;
            };
            result
                .entry(memory_id.0.clone())
                .or_default()
                .groups
                .insert(group_id.clone());
        }
        for link in response.dedup_links {
            let (Some(memory_id), Some(dedup_group_id)) = (
                memories.get(&link.from_node),
                dedup_groups.get(&link.to_node),
            ) else {
                continue;
            };
            result
                .entry(memory_id.0.clone())
                .or_default()
                .dedup_groups
                .insert(dedup_group_id.clone());
        }
        Ok(result)
    }

    /// Stable labels for background jobs that must never merge memories across
    /// RBAC dedup domains.
    pub async fn memory_security_domains(
        &self,
        memory_ids: &[String],
    ) -> Result<HashMap<String, String>> {
        Ok(self
            .memory_scope_map(memory_ids)
            .await?
            .into_iter()
            .map(|(memory_id, stored)| {
                let domain = if !stored.rbac_scope.is_empty() {
                    stored.rbac_scope.clone()
                } else if stored.dedup_groups.len() == 1 {
                    stored
                        .dedup_groups
                        .iter()
                        .next()
                        .map(|id| format!("dedup:{id}"))
                        .unwrap_or_else(|| "invalid:missing-dedup-group".to_string())
                } else if stored.dedup_groups.len() > 1 {
                    "invalid:multiple-dedup-groups".to_string()
                } else if stored.groups.len() == 1 {
                    stored
                        .groups
                        .iter()
                        .next()
                        .map(|id| format!("group:{id}"))
                        .unwrap_or_else(|| "invalid:missing-group".to_string())
                } else if stored.groups.is_empty() {
                    "unscoped".to_string()
                } else {
                    let mut groups = stored.groups.into_iter().collect::<Vec<_>>();
                    groups.sort();
                    format!("invalid:groups:{}", groups.join(","))
                };
                (memory_id, domain)
            })
            .collect())
    }

    /// Whether an in-place update would leak a post-membership-change value to
    /// groups that only retain historical access. Such writes must create a
    /// new version in the current scope instead.
    pub async fn memory_requires_fork_for_scope(
        &self,
        memory_id: &str,
        scope: &RbacMemoryScope,
    ) -> Result<bool> {
        let RbacMemoryScope::DedupGroup {
            dedup_group_id,
            group_ids,
        } = scope
        else {
            return Ok(false);
        };
        let stored = self
            .memory_scope_map(&[memory_id.to_string()])
            .await?
            .remove(memory_id)
            .unwrap_or_default();
        Ok(stored.dedup_groups.contains(dedup_group_id)
            && stored.groups.iter().cloned().collect::<BTreeSet<_>>() != *group_ids)
    }

    async fn is_historical_federation_memory(&self, memory_id: &str) -> Result<bool> {
        let stored = self
            .memory_scope_map(&[memory_id.to_string()])
            .await?
            .remove(memory_id)
            .unwrap_or_default();
        let Some(dedup_group_id) = stored.dedup_groups.iter().next() else {
            return Ok(false);
        };
        if stored.dedup_groups.len() != 1 {
            return Ok(true);
        }
        let current = self
            .snapshot()
            .await?
            .groups
            .into_iter()
            .filter(|(_, group)| group.dedup_group_id.as_deref() == Some(dedup_group_id))
            .map(|(group_id, _)| group_id)
            .collect::<HashSet<_>>();
        Ok(stored.groups != current)
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
        self.authorize_write_for_group(actor, owner, None).await
    }

    pub async fn authorize_write_for_group(
        &self,
        actor: &str,
        owner: &str,
        group_id: Option<&str>,
    ) -> Result<()> {
        let policy = self.snapshot().await?;
        if !policy.can_create_for_group(actor, owner, group_id) {
            bail!(
                "RBAC denied write for '{actor}' as owner '{owner}' in group '{}'",
                group_id.unwrap_or("<unscoped>")
            )
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

    pub async fn authorize_memory_write(
        &self,
        actor: &str,
        owner: &str,
        memory_id: &str,
    ) -> Result<()> {
        let policy = self.snapshot().await?;
        if !policy.enabled {
            return Ok(());
        }
        if self.is_historical_federation_memory(memory_id).await? {
            bail!(
                "RBAC denied in-place update for historical federation memory '{memory_id}'; create a new version instead"
            )
        }
        if policy.is_admin(actor) {
            return Ok(());
        }
        let ids = vec![memory_id.to_string()];
        let groups = self
            .memory_group_map(&ids)
            .await?
            .remove(memory_id)
            .unwrap_or_default();
        if !policy.can_write_memory(actor, owner, &groups) {
            bail!("RBAC denied write for '{actor}' on memory '{memory_id}'")
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct MemoryRbacNode {
    id: String,
    memory_id: String,
    #[serde(default)]
    rbac_scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroupRbacNode {
    id: String,
    group_id: String,
}

#[derive(Debug, Deserialize)]
struct DedupGroupRbacNode {
    id: String,
    dedup_group_id: String,
}

#[derive(Debug, Deserialize)]
struct MemoryRbacLink {
    from_node: String,
    to_node: String,
}

#[derive(Debug, Default, Deserialize)]
struct MemoryRbacScopesResponse {
    #[serde(default)]
    memories: Vec<MemoryRbacNode>,
    #[serde(default)]
    group_links: Vec<MemoryRbacLink>,
    #[serde(default)]
    groups: Vec<GroupRbacNode>,
    #[serde(default)]
    dedup_links: Vec<MemoryRbacLink>,
    #[serde(default)]
    dedup_groups: Vec<DedupGroupRbacNode>,
}

#[derive(Debug, Clone, Default)]
struct StoredMemoryScope {
    rbac_scope: String,
    groups: HashSet<String>,
    dedup_groups: HashSet<String>,
}

#[derive(Debug, Deserialize)]
struct DedupGroupMemoryNode {
    #[serde(default)]
    memory_id: String,
}

#[derive(Debug, Default, Deserialize)]
struct DedupGroupMemoriesResponse {
    #[serde(default)]
    memories: Vec<DedupGroupMemoryNode>,
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

fn apply_dedup_memberships(policy: &mut RbacPolicy, value: &serde_json::Value) {
    let groups = rows(value, "groups")
        .into_iter()
        .filter_map(|row| Some((string_field(row, "id")?, string_field(row, "group_id")?)))
        .collect::<HashMap<_, _>>();
    let dedup_groups = rows(value, "dedup_groups")
        .into_iter()
        .filter_map(|row| {
            Some((
                string_field(row, "id")?,
                string_field(row, "dedup_group_id")?,
            ))
        })
        .collect::<HashMap<_, _>>();
    for link in rows(value, "links") {
        let (Some(from), Some(to)) = (
            string_field(link, "from_node"),
            string_field(link, "to_node"),
        ) else {
            continue;
        };
        let (Some(group_id), Some(dedup_group_id)) = (groups.get(&from), dedup_groups.get(&to))
        else {
            continue;
        };
        if policy.dedup_groups.contains_key(dedup_group_id)
            && let Some(group) = policy.groups.get_mut(group_id)
        {
            group.dedup_group_id = Some(dedup_group_id.clone());
        }
    }
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
    fn multi_group_owner_requires_explicit_write_group() {
        let mut p = sample();
        p.assign_group("worker", "beta", Role::Worker).unwrap();
        p.assign_group("beta-viewer", "beta", Role::Viewer).unwrap();

        assert!(p.can_create_for_group("worker", "worker", Some("alpha")));
        assert!(p.can_create_for_group("worker", "worker", Some("beta")));
        assert!(!p.can_create_for_group("worker", "worker", None));
        assert!(!p.can_create_for_group("viewer", "viewer", Some("alpha")));

        let alpha_only = HashSet::from(["alpha".to_string()]);
        assert!(p.can_write_memory("worker", "worker", &alpha_only));
        assert!(!p.can_write_memory("beta-viewer", "worker", &alpha_only));
    }

    #[test]
    fn unscoped_enabled_memory_is_admin_only() {
        let p = sample();
        let unscoped = HashSet::new();
        assert!(p.can_write_memory("root", "worker", &unscoped));
        assert!(!p.can_write_memory("worker", "worker", &unscoped));
        assert!(p.can_create_for_group("root", "worker", None));
        assert!(!p.can_create_for_group("worker", "worker", None));
    }

    #[test]
    fn dedup_federation_shares_one_scope_across_current_groups() {
        let mut p = sample();
        p.upsert_dedup_group("development", "Engineering knowledge");
        p.assign_dedup_group("alpha", Some("development")).unwrap();
        p.assign_dedup_group("beta", Some("development")).unwrap();

        let alpha = p.resolve_memory_scope(Some("alpha")).unwrap();
        let beta = p.resolve_memory_scope(Some("beta")).unwrap();
        assert_eq!(alpha.fingerprint_scope(), beta.fingerprint_scope());
        assert_eq!(
            alpha.group_ids(),
            BTreeSet::from(["alpha".to_string(), "beta".to_string()])
        );
    }

    #[test]
    fn leaving_federation_isolates_new_writes_without_erasing_history() {
        let mut p = sample();
        p.upsert_dedup_group("development", "Engineering knowledge");
        p.assign_dedup_group("alpha", Some("development")).unwrap();
        p.assign_dedup_group("beta", Some("development")).unwrap();
        let historical = p.resolve_memory_scope(Some("beta")).unwrap();

        p.assign_dedup_group("beta", None).unwrap();
        let future = p.resolve_memory_scope(Some("beta")).unwrap();
        assert_ne!(historical.fingerprint_scope(), future.fingerprint_scope());
        assert_eq!(future.group_ids(), BTreeSet::from(["beta".to_string()]));
        assert_eq!(
            p.resolve_memory_scope(Some("alpha")).unwrap().group_ids(),
            BTreeSet::from(["alpha".to_string()])
        );
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
