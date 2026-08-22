//! Resumable server-side placement of enrolled clients into working groups.

use super::*;

/// Desired final placement for a principal admitted through `onboarding`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientWorkspaceOnboarding {
    pub principal_id: String,
    pub group_id: String,
    /// Required only when the target group does not exist yet.
    pub group_name: Option<String>,
    pub group_description: String,
    pub role: Role,
    /// Keep temporary `onboarding` visibility after the working grant.
    pub keep_onboarding: bool,
}

/// Verified graph state after a client workspace onboarding run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientWorkspaceOnboardingReport {
    pub principal_id: String,
    pub group_id: String,
    pub group_created: bool,
    pub requested_role: String,
    pub active_roles: Vec<String>,
    pub onboarding_active: bool,
    pub onboarding_roles_revoked: Vec<String>,
    pub readable_groups: BTreeSet<String>,
    pub can_write_own_memories: bool,
    pub memory_scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dedup_group_id: Option<String>,
}

impl RbacManager {
    /// Converge one enrolled client into a working workspace.
    ///
    /// The mutation order is intentionally resumable: create the group when
    /// needed, grant the working role, then remove temporary onboarding access.
    /// A failure therefore never removes the principal's only access before the
    /// intended grant exists, and rerunning the same request safely completes it.
    pub async fn onboard_client_to_workspace_as(
        &self,
        request: &ClientWorkspaceOnboarding,
        actor: &str,
    ) -> Result<ClientWorkspaceOnboardingReport> {
        self.authorize_admin(actor).await?;
        validate_request(request)?;

        let initial = self.snapshot().await?;
        if !initial.enabled || initial.migration_state != RbacMigrationState::Active {
            bail!("RBAC onboarding is not active on this Helixir node");
        }
        if !self
            .reserved_registered_user_ids()
            .await?
            .contains(&request.principal_id)
        {
            bail!(
                "principal '{}' must first connect through reserved onboarding",
                request.principal_id
            );
        }

        let group_created = if initial.groups.contains_key(&request.group_id) {
            false
        } else {
            if request.group_id == crate::core::rbac_compat::DEFAULT_GROUP_ID {
                bail!("reserved default workspace is missing; run `helixir rbac bootstrap`");
            }
            let name = request
                .group_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "group '{}' does not exist; provide --group-name to create it",
                        request.group_id
                    )
                })?;
            self.create_group_as(
                &request.group_id,
                name,
                request.group_description.trim(),
                actor,
            )
            .await?;
            true
        };

        self.add_user_to_group(
            &request.principal_id,
            &request.group_id,
            request.role,
            actor,
        )
        .await?;

        let onboarding_roles_revoked = if request.keep_onboarding {
            Vec::new()
        } else {
            self.remove_user_from_group(
                &request.principal_id,
                crate::core::rbac_compat::ONBOARDING_GROUP_ID,
                actor,
            )
            .await?
        };

        let policy = self.snapshot().await?;
        let binding = policy
            .users
            .get(&request.principal_id)
            .ok_or_else(|| anyhow::anyhow!("principal disappeared after RBAC onboarding"))?;
        let target_roles = binding
            .groups
            .get(&request.group_id)
            .ok_or_else(|| anyhow::anyhow!("target group grant was not persisted"))?;
        if !target_roles.contains(&request.role) {
            bail!("requested target role was not persisted");
        }
        let onboarding_active = binding
            .groups
            .get(crate::core::rbac_compat::ONBOARDING_GROUP_ID)
            .is_some_and(|roles| !roles.is_empty());
        if !request.keep_onboarding && onboarding_active {
            bail!("temporary onboarding membership is still active");
        }

        let readable_groups = policy
            .readable_groups(&request.principal_id)
            .unwrap_or_else(|| policy.groups.keys().cloned().collect())
            .into_iter()
            .collect::<BTreeSet<_>>();
        let memory_scope = policy.resolve_memory_scope(Some(&request.group_id))?;
        let dedup_group_id = policy.group(&request.group_id)?.dedup_group_id.clone();

        Ok(ClientWorkspaceOnboardingReport {
            principal_id: request.principal_id.clone(),
            group_id: request.group_id.clone(),
            group_created,
            requested_role: request.role.label().to_string(),
            active_roles: target_roles
                .iter()
                .map(|role| role.label().to_string())
                .collect(),
            onboarding_active,
            onboarding_roles_revoked,
            readable_groups,
            can_write_own_memories: policy.can_create_for_group(
                &request.principal_id,
                &request.principal_id,
                Some(&request.group_id),
            ),
            memory_scope: memory_scope_label(&memory_scope),
            dedup_group_id,
        })
    }
}

fn validate_request(request: &ClientWorkspaceOnboarding) -> Result<()> {
    if request.principal_id.trim().is_empty() {
        bail!("principal id cannot be empty");
    }
    if request.group_id.trim().is_empty() {
        bail!("target group id cannot be empty");
    }
    if matches!(
        request.group_id.as_str(),
        crate::core::rbac_compat::ONBOARDING_GROUP_ID | crate::core::rbac_compat::MOIRAI_GROUP_ID
    ) {
        bail!("target workspace must be a working group or reserved default");
    }
    if matches!(request.role, Role::Admin | Role::TeamLead) {
        bail!("client workspace role must be groupadmin, moderator, worker, or viewer");
    }
    Ok(())
}

fn memory_scope_label(scope: &RbacMemoryScope) -> String {
    match scope {
        RbacMemoryScope::Legacy => "legacy".to_string(),
        RbacMemoryScope::Unscoped => "admin-only".to_string(),
        RbacMemoryScope::CompatibilityGroup { group_id } => {
            format!("compatibility:{group_id}")
        }
        RbacMemoryScope::Group { group_id } => format!("group:{group_id}"),
        RbacMemoryScope::DedupGroup { dedup_group_id, .. } => {
            format!("dedup:{dedup_group_id}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(group_id: &str, role: Role) -> ClientWorkspaceOnboarding {
        ClientWorkspaceOnboarding {
            principal_id: "client-a".to_string(),
            group_id: group_id.to_string(),
            group_name: Some("Development".to_string()),
            group_description: String::new(),
            role,
            keep_onboarding: false,
        }
    }

    #[test]
    fn rejects_reserved_targets_and_non_group_roles() {
        assert!(validate_request(&request("onboarding", Role::Worker)).is_err());
        assert!(validate_request(&request("moirai", Role::Worker)).is_err());
        assert!(validate_request(&request("development", Role::Admin)).is_err());
        assert!(validate_request(&request("development", Role::TeamLead)).is_err());
        assert!(validate_request(&request("default", Role::GroupAdmin)).is_ok());
    }

    #[test]
    fn renders_stable_security_scope_labels() {
        assert_eq!(
            memory_scope_label(&RbacMemoryScope::Group {
                group_id: "development".to_string(),
            }),
            "group:development"
        );
        assert_eq!(
            memory_scope_label(&RbacMemoryScope::DedupGroup {
                dedup_group_id: "engineering".to_string(),
                group_ids: BTreeSet::new(),
            }),
            "dedup:engineering"
        );
    }
}
