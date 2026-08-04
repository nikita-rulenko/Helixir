//! Memory visibility, access checks, and scoped dedup operations.

use super::*;

impl RbacManager {
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
                        && match scope {
                            RbacMemoryScope::CompatibilityGroup { group_id } => {
                                stored.groups.contains(group_id)
                            }
                            _ => true,
                        }
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

    /// Return memories that predate RBAC materialization and therefore need
    /// migration into the compatibility group.
    pub(crate) async fn legacy_unscoped_memory_ids(
        &self,
        memory_ids: &[String],
    ) -> Result<HashSet<String>> {
        Ok(self
            .memory_scope_map(memory_ids)
            .await?
            .into_iter()
            .filter_map(|(memory_id, scope)| scope.is_legacy_unscoped().then_some(memory_id))
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
        let policy = self.snapshot().await?;
        if policy.enabled && !policy.is_admin(revoked_by) {
            bail!("RBAC management requires a global admin");
        }
        let group = group_id.unwrap_or("");
        ensure_admin_revoke_is_recoverable(&policy, subject_id, role, group)?;
        let query = if group.is_empty() {
            "revokeRbacRole"
        } else {
            "revokeRbacGroupRole"
        };
        self.db
            .execute_query::<serde_json::Value, _>(
                query,
                &serde_json::json!({
                    "subject_id": subject_id,
                    "role": role.label(),
                    "group_id": group,
                    "assignment_id": assignment_id(subject_id, group, role),
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
