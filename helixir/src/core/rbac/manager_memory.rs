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

    pub(crate) async fn unlink_memory_from_group(
        &self,
        memory_id: &str,
        group_id: &str,
    ) -> Result<()> {
        self.db
            .execute_query::<serde_json::Value, _>(
                "unlinkMemoryFromRbacGroup",
                &serde_json::json!({"memory_id": memory_id, "group_id": group_id}),
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

    /// Scan-free counterpart for hot search/write paths that already carry
    /// each memory's HelixDB primary key.
    pub async fn memory_refs_in_scope(
        &self,
        scope: &RbacMemoryScope,
        memory_refs: &[(String, String)],
    ) -> Result<Option<HashSet<String>>> {
        if matches!(scope, RbacMemoryScope::Legacy) {
            return Ok(None);
        }
        if memory_refs.is_empty() {
            return Ok(Some(HashSet::new()));
        }
        let scope_map = self.memory_scope_map_by_refs(memory_refs).await?;
        let expected_scope = scope.fingerprint_scope().unwrap_or_default();
        Ok(Some(
            memory_refs
                .iter()
                .filter(|(memory_id, _)| {
                    let stored = scope_map.get(memory_id).cloned().unwrap_or_default();
                    stored.rbac_scope == expected_scope
                        && match scope {
                            RbacMemoryScope::CompatibilityGroup { group_id } => {
                                stored.groups.contains(group_id)
                            }
                            _ => true,
                        }
                })
                .map(|(memory_id, _)| memory_id.clone())
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

    /// Scan-free visibility filtering for search results carrying HelixDB
    /// primary keys. Global admins still return `None` without projections.
    pub async fn visible_memory_refs(
        &self,
        actor: &str,
        memory_refs: &[(String, String)],
    ) -> Result<Option<HashSet<String>>> {
        let policy = self.snapshot().await?;
        let Some(readable_groups) = policy.readable_groups(actor) else {
            return Ok(None);
        };
        if readable_groups.is_empty() || memory_refs.is_empty() {
            return Ok(Some(HashSet::new()));
        }
        let group_map = self.memory_scope_map_by_refs(memory_refs).await?;
        Ok(Some(
            memory_refs
                .iter()
                .filter(|(memory_id, _)| {
                    group_map
                        .get(memory_id)
                        .is_some_and(|scope| !scope.groups.is_disjoint(&readable_groups))
                })
                .map(|(memory_id, _)| memory_id.clone())
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

    /// Return pre-two-workspace memories that still need to converge on
    /// `default`. This includes the earlier one-space rollout where legacy
    /// rows were attached only to `onboarding`.
    pub(crate) async fn memories_requiring_default_migration(
        &self,
        memory_ids: &[String],
    ) -> Result<HashSet<String>> {
        Ok(self
            .memory_scope_map(memory_ids)
            .await?
            .into_iter()
            .filter_map(|(memory_id, scope)| {
                scope
                    .needs_default_workspace_migration()
                    .then_some(memory_id)
            })
            .collect())
    }

    pub(super) async fn memory_scope_map(
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

        Ok(parse_memory_scope_response(response))
    }

    async fn memory_scope_map_by_refs(
        &self,
        memory_refs: &[(String, String)],
    ) -> Result<HashMap<String, StoredMemoryScope>> {
        let mut merged = MemoryRbacScopesResponse::default();
        for (_, internal_id) in memory_refs {
            let response: MemoryRbacScopeResponse = self
                .db
                .execute_query(
                    "getMemoryRbacScopeByInternalId",
                    &serde_json::json!({ "internal_id": internal_id }),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            merged.append_single(response);
        }
        Ok(parse_memory_scope_response(merged))
    }
}

fn parse_memory_scope_response(
    response: MemoryRbacScopesResponse,
) -> HashMap<String, StoredMemoryScope> {
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
    result
}
