//! Memory-domain checks and write authorization.

use super::*;

impl RbacManager {
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
        let policy = self.snapshot().await?;
        let group = group_id.unwrap_or("");
        if group.is_empty() {
            if policy.enabled && !policy.is_admin(revoked_by) {
                bail!("RBAC management requires a global admin");
            }
        } else if !policy.can_manage_group(revoked_by, group) {
            bail!("RBAC group management for '{group}' requires its groupadmin or a global admin");
        }
        ensure_admin_revoke_is_recoverable(&policy, subject_id, role, group)?;
        let revoked_at = Utc::now().to_rfc3339();
        if group.is_empty() {
            self.db
                .execute_query::<serde_json::Value, _>(
                    "revokeRbacRole",
                    &serde_json::json!({
                        "subject_id": subject_id,
                        "role": role.label(),
                        "group_id": group,
                        "revoked_at": revoked_at,
                        "updated_by": revoked_by,
                    }),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            return Ok(());
        }

        let stored: serde_json::Value = self
            .db
            .execute_query("getAllRbacAssignments", &serde_json::json!({}))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let assignments = rows(&stored, "assignments")
            .into_iter()
            .filter(|assignment| {
                assignment.get("active").and_then(serde_json::Value::as_i64) == Some(1)
            })
            .filter(|assignment| {
                string_field(assignment, "subject_id").as_deref() == Some(subject_id)
            })
            .filter(|assignment| string_field(assignment, "group_id").as_deref() == Some(group))
            .collect::<Vec<_>>();
        if !assignments
            .iter()
            .any(|assignment| string_field(assignment, "role").as_deref() == Some(role.label()))
        {
            return Ok(());
        }
        let role_priority = |candidate: Role| match candidate {
            Role::GroupAdmin => 5,
            Role::Moderator => 4,
            Role::Worker => 3,
            Role::Viewer => 2,
            Role::TeamLead => 1,
            Role::Admin => 0,
        };
        let replacement = assignments
            .into_iter()
            .filter_map(|assignment| {
                let candidate = Role::parse(&string_field(assignment, "role")?)?;
                (candidate != role).then_some((assignment, candidate))
            })
            .max_by_key(|(_, candidate)| role_priority(*candidate));
        let (
            membership_assignment_id,
            membership_role,
            membership_granted_by,
            membership_granted_at,
            membership_active,
        ) = replacement.map_or_else(
            || {
                (
                    String::new(),
                    String::new(),
                    revoked_by.to_string(),
                    revoked_at.clone(),
                    0i64,
                )
            },
            |(assignment, candidate)| {
                (
                    string_field(assignment, "assignment_id").unwrap_or_default(),
                    candidate.label().to_string(),
                    string_field(assignment, "granted_by").unwrap_or_default(),
                    string_field(assignment, "created_at").unwrap_or_default(),
                    1i64,
                )
            },
        );
        self.db
            .execute_query::<serde_json::Value, _>(
                "revokeRbacGroupRole",
                &serde_json::json!({
                    "subject_id": subject_id,
                    "role": role.label(),
                    "group_id": group,
                    "membership_assignment_id": membership_assignment_id,
                    "membership_role": membership_role,
                    "membership_granted_by": membership_granted_by,
                    "membership_granted_at": membership_granted_at,
                    "membership_active": membership_active,
                    "revoked_at": revoked_at,
                    "updated_by": revoked_by,
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
