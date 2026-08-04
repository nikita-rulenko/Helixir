//! One-way bootstrap into the default RBAC security model.
//!
//! `default` preserves the historical shared data plane. `onboarding` is a
//! separate admission boundary for principals discovered after migration.

use std::collections::BTreeSet;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use super::rbac::{
    RbacManager, RbacMemoryScope, RbacMigrationKind, RbacMigrationState, RbacPolicy, Role,
};

/// Reserved admission group for newly discovered principals.
pub const ONBOARDING_GROUP_ID: &str = "onboarding";
/// Reserved full-trust workspace containing pre-RBAC knowledge and principals.
pub const DEFAULT_GROUP_ID: &str = "default";
const DEFAULT_COMPATIBILITY_GROUP_NAME: &str = "Default";
const DEFAULT_COMPATIBILITY_GROUP_DESCRIPTION: &str =
    "Administrative workspace for pre-RBAC shared knowledge";
const ONBOARDING_GROUP_NAME: &str = "Onboarding";
const ONBOARDING_GROUP_DESCRIPTION: &str =
    "Admission workspace for newly discovered users and agents";
const VERIFY_BATCH_SIZE: usize = 256;

/// One write authorization result produced from a single policy snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedWriteScope {
    /// Concrete group persisted with buffered writes and materialized as an ACL edge.
    pub group_id: Option<String>,
    /// Deduplication and visibility domain for the write pipeline.
    pub scope: RbacMemoryScope,
}

/// Observable outcome of an idempotent compatibility bootstrap.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompatibilityBootstrapReport {
    pub enabled_before: bool,
    pub enabled_after: bool,
    pub operator_id: String,
    pub group_id: String,
    pub onboarding_group_id: String,
    pub migration_kind: RbacMigrationKind,
    pub principals_enrolled: Vec<String>,
    pub users_registered: usize,
    pub memories_seen: usize,
    pub memories_attached: usize,
}

impl RbacPolicy {
    /// Infer an omitted group only when exactly one reserved workspace is
    /// writable. Explicit groups always win; ambiguous membership fails closed.
    pub fn effective_write_group(&self, actor: &str, requested: Option<&str>) -> Option<String> {
        if let Some(group_id) = requested {
            return Some(group_id.to_string());
        }
        if !self.enabled {
            return None;
        }
        let binding = self.users.get(actor)?;
        [DEFAULT_GROUP_ID, ONBOARDING_GROUP_ID]
            .into_iter()
            .filter(|group_id| {
                binding.groups.get(*group_id).is_some_and(|roles| {
                    roles.iter().any(|role| {
                        matches!(role, Role::GroupAdmin | Role::Moderator | Role::Worker)
                    })
                })
            })
            .map(str::to_string)
            .reduce(|_, _| String::new())
            .filter(|group_id| !group_id.is_empty())
    }
}

impl RbacManager {
    /// Authorize and resolve a write using one policy snapshot so permission
    /// checks cannot disagree with the scope used by the write pipeline.
    pub async fn authorize_and_resolve_write_scope(
        &self,
        actor: &str,
        owner: &str,
        requested_group: Option<&str>,
    ) -> Result<AuthorizedWriteScope> {
        let policy = self.snapshot().await?;
        let group_id = policy.effective_write_group(actor, requested_group);
        if !policy.can_create_for_group(actor, owner, group_id.as_deref()) {
            bail!(
                "RBAC denied write for '{actor}' as owner '{owner}' in group '{}'",
                group_id.as_deref().unwrap_or("<unscoped>")
            );
        }
        let scope = policy.resolve_memory_scope(group_id.as_deref())?;
        Ok(AuthorizedWriteScope { group_id, scope })
    }

    /// Converge a fresh or legacy store on the permanent two-workspace model.
    /// Retrying is safe because the migration kind is checkpointed before any
    /// principal or memory is reclassified and every mutation is idempotent.
    pub async fn bootstrap_compatibility(
        &self,
        operator_id: &str,
        principals: &[String],
    ) -> Result<CompatibilityBootstrapReport> {
        let operator_id = operator_id.trim();
        if operator_id.is_empty() {
            bail!("RBAC bootstrap operator id cannot be empty");
        }

        let initial = self.snapshot().await?;
        if initial.enabled && !initial.is_admin(operator_id) {
            bail!("RBAC bootstrap requires an existing global admin while enforcement is enabled");
        }
        let mut enrolled = principals
            .iter()
            .map(|principal| principal.trim())
            .filter(|principal| !principal.is_empty())
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        enrolled.insert(operator_id.to_string());

        let existing_users = self.all_user_ids().await?;
        let memory_ids = self.all_memory_ids().await?;
        let migration_kind = initial.migration_kind.unwrap_or({
            if initial.enabled || !existing_users.is_empty() || !memory_ids.is_empty() {
                RbacMigrationKind::Legacy
            } else {
                RbacMigrationKind::Fresh
            }
        });

        self.create_group_as(
            DEFAULT_GROUP_ID,
            DEFAULT_COMPATIBILITY_GROUP_NAME,
            DEFAULT_COMPATIBILITY_GROUP_DESCRIPTION,
            operator_id,
        )
        .await?;
        self.create_group_as(
            ONBOARDING_GROUP_ID,
            ONBOARDING_GROUP_NAME,
            ONBOARDING_GROUP_DESCRIPTION,
            operator_id,
        )
        .await?;
        self.grant(operator_id, Role::Admin, None, operator_id)
            .await?;
        let transition_active = initial.migration_state == RbacMigrationState::Active;
        if !transition_active {
            self.set_migration_state(RbacMigrationState::Migrating, migration_kind, operator_id)
                .await?;
        }

        let mut legacy_principals = existing_users.iter().cloned().collect::<BTreeSet<_>>();
        legacy_principals.extend(enrolled.iter().cloned());
        if transition_active {
            let policy = self.snapshot().await?;
            for principal in &enrolled {
                let already_registered = policy.users.get(principal).is_some_and(|binding| {
                    binding.global_roles.contains(&Role::Admin)
                        || [DEFAULT_GROUP_ID, ONBOARDING_GROUP_ID]
                            .into_iter()
                            .any(|group| {
                                binding
                                    .groups
                                    .get(group)
                                    .is_some_and(|roles| !roles.is_empty())
                            })
                });
                if !already_registered {
                    self.grant(
                        principal,
                        Role::Worker,
                        Some(ONBOARDING_GROUP_ID),
                        operator_id,
                    )
                    .await?;
                }
            }
        } else {
            match migration_kind {
                RbacMigrationKind::Legacy => {
                    for principal in &legacy_principals {
                        self.grant(
                            principal,
                            Role::GroupAdmin,
                            Some(DEFAULT_GROUP_ID),
                            operator_id,
                        )
                        .await?;
                    }
                    let policy = self.snapshot().await?;
                    for principal in &legacy_principals {
                        let roles = policy
                            .users
                            .get(principal)
                            .and_then(|binding| binding.groups.get(ONBOARDING_GROUP_ID))
                            .cloned()
                            .unwrap_or_default();
                        for role in roles {
                            self.revoke_as(principal, role, Some(ONBOARDING_GROUP_ID), operator_id)
                                .await?;
                        }
                    }
                }
                RbacMigrationKind::Fresh => {
                    let policy = self.snapshot().await?;
                    for principal in &enrolled {
                        let already_assigned = policy.users.get(principal).is_some_and(|binding| {
                            binding.groups.values().any(|roles| !roles.is_empty())
                        });
                        if !already_assigned {
                            self.grant(
                                principal,
                                Role::Worker,
                                Some(ONBOARDING_GROUP_ID),
                                operator_id,
                            )
                            .await?;
                        }
                    }
                }
            }
        }

        let first_pass = if !transition_active && migration_kind == RbacMigrationKind::Legacy {
            self.attach_legacy_memories_to_default(operator_id).await?
        } else {
            0
        };
        if !initial.enabled {
            self.enable(operator_id).await?;
        }
        let second_pass = if !transition_active && migration_kind == RbacMigrationKind::Legacy {
            self.attach_legacy_memories_to_default(operator_id).await?
        } else {
            0
        };
        let (memories_seen, missing) = self.compatibility_memory_coverage().await?;
        if !missing.is_empty() {
            bail!(
                "RBAC bootstrap verification failed: {} legacy memories are outside 'default'",
                missing.len()
            );
        }
        let policy = self.snapshot().await?;
        if !policy.enabled
            || !policy.is_admin(operator_id)
            || !policy.groups.contains_key(DEFAULT_GROUP_ID)
            || !policy.groups.contains_key(ONBOARDING_GROUP_ID)
        {
            bail!("RBAC bootstrap verification failed: policy graph is incomplete");
        }
        for principal in &enrolled {
            let ready = policy.users.get(principal).is_some_and(|binding| {
                binding.global_roles.contains(&Role::Admin)
                    || [DEFAULT_GROUP_ID, ONBOARDING_GROUP_ID]
                        .into_iter()
                        .any(|group| {
                            binding.groups.get(group).is_some_and(|roles| {
                                roles.contains(&Role::Worker) || roles.contains(&Role::GroupAdmin)
                            })
                        })
            });
            if !ready {
                bail!("RBAC bootstrap verification failed for principal '{principal}'");
            }
        }
        if !transition_active {
            self.set_migration_state(RbacMigrationState::Active, migration_kind, operator_id)
                .await?;
        }
        Ok(CompatibilityBootstrapReport {
            enabled_before: initial.enabled,
            enabled_after: true,
            operator_id: operator_id.to_string(),
            group_id: DEFAULT_GROUP_ID.to_string(),
            onboarding_group_id: ONBOARDING_GROUP_ID.to_string(),
            migration_kind,
            principals_enrolled: enrolled.into_iter().collect(),
            users_registered: existing_users.len(),
            memories_seen,
            memories_attached: first_pass.saturating_add(second_pass),
        })
    }

    async fn attach_legacy_memories_to_default(&self, actor: &str) -> Result<usize> {
        let ids = self.all_memory_ids().await?;
        let mut attached = 0usize;
        for batch in ids.chunks(VERIFY_BATCH_SIZE) {
            let legacy_ids = self.memories_requiring_default_migration(batch).await?;
            for memory_id in legacy_ids {
                self.link_memory_to_group(&memory_id, Some(DEFAULT_GROUP_ID), actor)
                    .await?;
                self.unlink_memory_from_group(&memory_id, ONBOARDING_GROUP_ID)
                    .await?;
                attached += 1;
            }
        }
        Ok(attached)
    }

    async fn compatibility_memory_coverage(&self) -> Result<(usize, Vec<String>)> {
        let ids = self.all_memory_ids().await?;
        if self.snapshot().await?.migration_kind == Some(RbacMigrationKind::Fresh) {
            return Ok((ids.len(), Vec::new()));
        }
        let mut missing = Vec::new();
        for batch in ids.chunks(VERIFY_BATCH_SIZE) {
            missing.extend(self.memories_requiring_default_migration(batch).await?);
        }
        Ok((ids.len(), missing))
    }

    /// Check whether every legacy trusted-mode memory has been materialized into
    /// the reserved group without widening already-scoped RBAC memories.
    pub async fn compatibility_coverage_complete(&self) -> Result<bool> {
        Ok(self.compatibility_memory_coverage().await?.1.is_empty())
    }

    /// Check whether the onboarding registry has been initialized.
    ///
    /// `User` nodes are also provenance owners and can legitimately be created
    /// after bootstrap by an authorized on-behalf write. They are not principals
    /// until admitted through onboarding, so doctor must not require every User
    /// node to have a role. Bootstrap itself verifies the exact pre-transition
    /// user set before it reports success.
    pub async fn compatibility_user_coverage_complete(&self) -> Result<bool> {
        let policy = self.snapshot().await?;
        let registered = self.reserved_registered_user_ids().await?;
        Ok(policy.groups.contains_key(DEFAULT_GROUP_ID)
            && policy.groups.contains_key(ONBOARDING_GROUP_ID)
            && !registered.is_empty())
    }

    async fn all_memory_ids(&self) -> Result<Vec<String>> {
        let response: AllMemoriesResponse = self
            .db
            .execute_query("getAllMemories", &serde_json::json!({}))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(response
            .memories
            .into_iter()
            .filter_map(|memory| (!memory.memory_id.is_empty()).then_some(memory.memory_id))
            .collect())
    }
}

#[derive(Debug, Default, Deserialize)]
struct AllMemoriesResponse {
    #[serde(default)]
    memories: Vec<MemoryIdNode>,
}

#[derive(Debug, Deserialize)]
struct MemoryIdNode {
    #[serde(default)]
    memory_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::rbac::{Group, UserBinding};

    fn compatibility_policy() -> RbacPolicy {
        let mut policy = RbacPolicy {
            enabled: true,
            ..Default::default()
        };
        policy.groups.insert(
            DEFAULT_GROUP_ID.to_string(),
            Group {
                name: DEFAULT_COMPATIBILITY_GROUP_NAME.to_string(),
                description: String::new(),
                dedup_group_id: None,
            },
        );
        policy.users.insert(
            "agent".to_string(),
            UserBinding {
                global_roles: BTreeSet::new(),
                groups: [
                    (
                        ONBOARDING_GROUP_ID.to_string(),
                        BTreeSet::from([Role::GroupAdmin]),
                    ),
                    (
                        DEFAULT_GROUP_ID.to_string(),
                        BTreeSet::from([Role::GroupAdmin]),
                    ),
                ]
                .into_iter()
                .collect(),
            },
        );
        policy
    }

    #[test]
    fn compatibility_group_keeps_legacy_fingerprint_and_adds_visibility() {
        let policy = compatibility_policy();
        let scope = policy.resolve_memory_scope(Some(DEFAULT_GROUP_ID)).unwrap();
        assert_eq!(scope.fingerprint_scope(), None);
        assert_eq!(
            scope.group_ids(),
            BTreeSet::from([DEFAULT_GROUP_ID.to_string()])
        );
    }

    #[test]
    fn omitted_group_is_inferred_only_for_enrolled_writer() {
        let policy = compatibility_policy();
        assert_eq!(policy.effective_write_group("agent", None), None);
        let mut policy = policy;
        policy
            .users
            .get_mut("agent")
            .unwrap()
            .groups
            .remove(ONBOARDING_GROUP_ID);
        assert_eq!(
            policy.effective_write_group("agent", None).as_deref(),
            Some(DEFAULT_GROUP_ID)
        );
        assert_eq!(policy.effective_write_group("unknown", None), None);
        assert_eq!(
            policy
                .effective_write_group("agent", Some("explicit"))
                .as_deref(),
            Some("explicit")
        );
    }

    #[test]
    fn compatibility_group_admin_can_write_for_legacy_owner() {
        let policy = compatibility_policy();
        assert!(policy.can_create_for_group("agent", "legacy-owner", Some(DEFAULT_GROUP_ID)));
        assert!(policy.can_write_memory(
            "agent",
            "legacy-owner",
            &[DEFAULT_GROUP_ID.to_string()].into_iter().collect()
        ));
    }
}
