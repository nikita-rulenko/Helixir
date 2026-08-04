//! Graph-backed RBAC administration and snapshot loading.

use super::*;

/// HelixDB-backed RBAC service.  The service is intentionally small: all
/// callers (CLI, MCP and the public client facade) resolve the same snapshot
/// through these named queries, so no host-local policy can diverge.
#[derive(Clone)]
pub struct RbacManager {
    pub(crate) db: Arc<HelixClient>,
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

    /// Authorize an explicitly privileged low-level maintenance surface.
    pub async fn authorize_admin_surface(&self, actor: &str) -> Result<()> {
        self.authorize_admin(actor).await
    }

    pub async fn authorize_pending_read(
        &self,
        actor: &str,
        owner: &str,
        creator: &str,
    ) -> Result<()> {
        let policy = self.snapshot().await?;
        if !policy.can_read_pending(actor, owner, creator) {
            bail!("RBAC denied pending result read for '{actor}'")
        }
        Ok(())
    }

    pub async fn authorize_outbox_read(&self, actor: &str, owner: &str) -> Result<()> {
        let policy = self.snapshot().await?;
        if !policy.can_read_outbox(actor, owner) {
            bail!("RBAC denied outbox read for '{actor}' as owner '{owner}'")
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
        reject_reserved_group_mutation(group_id, "deactivate")?;
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
        reject_reserved_group_mutation(group_id, "attach to a dedup federation")?;
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
        reject_reserved_group_mutation(group_id, "detach from a dedup federation")?;
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
            if policy.enabled
                && group != crate::core::rbac_compat::ONBOARDING_GROUP_ID
                && policy
                    .users
                    .get(subject_id)
                    .and_then(|binding| {
                        binding
                            .groups
                            .get(crate::core::rbac_compat::ONBOARDING_GROUP_ID)
                    })
                    .is_none_or(|roles| roles.is_empty())
            {
                bail!(
                    "user '{subject_id}' must be enrolled in '{}' before receiving a role in '{group}'",
                    crate::core::rbac_compat::ONBOARDING_GROUP_ID
                );
            }
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
        if group.is_empty()
            && let Some(object) = params.as_object_mut()
        {
            object.remove("group_id");
        }
        self.db
            .execute_query::<serde_json::Value, _>(query, &params)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(())
    }
}
