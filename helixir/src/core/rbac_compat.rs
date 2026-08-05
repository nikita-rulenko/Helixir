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
    /// Memories audited during this invocation. Zero means an existing active
    /// migration checkpoint made another full-store audit unnecessary.
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

        if initial.migration_state == RbacMigrationState::Active {
            let migration_kind = initial
                .migration_kind
                .ok_or_else(|| anyhow::anyhow!("active RBAC migration has no persisted kind"))?;
            if !initial.enabled
                || !initial.groups.contains_key(DEFAULT_GROUP_ID)
                || !initial.groups.contains_key(ONBOARDING_GROUP_ID)
            {
                bail!("active RBAC migration checkpoint has an incomplete policy graph");
            }
            let mut changed = false;
            for principal in &enrolled {
                if !principal_ready(&initial, principal) {
                    self.grant(
                        principal,
                        Role::Worker,
                        Some(ONBOARDING_GROUP_ID),
                        operator_id,
                    )
                    .await?;
                    changed = true;
                }
            }
            let policy = if changed {
                self.snapshot().await?
            } else {
                initial.clone()
            };
            for principal in &enrolled {
                if !principal_ready(&policy, principal) {
                    bail!("RBAC bootstrap verification failed for principal '{principal}'");
                }
            }
            return Ok(CompatibilityBootstrapReport {
                enabled_before: true,
                enabled_after: true,
                operator_id: operator_id.to_string(),
                group_id: DEFAULT_GROUP_ID.to_string(),
                onboarding_group_id: ONBOARDING_GROUP_ID.to_string(),
                migration_kind,
                principals_enrolled: enrolled.into_iter().collect(),
                users_registered: policy.users.len(),
                memories_seen: 0,
                memories_attached: 0,
            });
        }

        let existing_users = self.all_user_ids().await?;
        let mut discovered_memory_ids = None;
        let migration_kind = match initial.migration_kind {
            Some(kind) => kind,
            None if initial.enabled || !existing_users.is_empty() => RbacMigrationKind::Legacy,
            None => {
                let ids = self.all_memory_ids().await?;
                let kind = if ids.is_empty() {
                    RbacMigrationKind::Fresh
                } else {
                    RbacMigrationKind::Legacy
                };
                discovered_memory_ids = Some(ids);
                kind
            }
        };

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
        self.set_migration_state(RbacMigrationState::Migrating, migration_kind, operator_id)
            .await?;

        let mut legacy_principals = existing_users.iter().cloned().collect::<BTreeSet<_>>();
        legacy_principals.extend(enrolled.iter().cloned());
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

        let first_ids = if migration_kind == RbacMigrationKind::Legacy {
            Some(match discovered_memory_ids {
                Some(ids) => ids,
                None => self.all_memory_ids().await?,
            })
        } else {
            None
        };
        let first_pass = match first_ids.as_deref() {
            Some(ids) => {
                self.attach_legacy_memories_to_default(ids, operator_id)
                    .await?
            }
            None => 0,
        };
        if !initial.enabled {
            self.enable(operator_id).await?;
        }
        let final_ids = if first_ids.is_some() {
            Some(self.all_memory_ids().await?)
        } else {
            None
        };
        let second_pass = match final_ids.as_deref() {
            Some(ids) => {
                self.attach_legacy_memories_to_default(ids, operator_id)
                    .await?
            }
            None => 0,
        };
        let (memories_seen, missing) = match final_ids.as_deref() {
            Some(ids) => {
                self.compatibility_memory_coverage(ids, migration_kind)
                    .await?
            }
            None => (0, Vec::new()),
        };
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
        self.set_migration_state(RbacMigrationState::Active, migration_kind, operator_id)
            .await?;
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

    async fn attach_legacy_memories_to_default(
        &self,
        ids: &[String],
        actor: &str,
    ) -> Result<usize> {
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

    async fn compatibility_memory_coverage(
        &self,
        ids: &[String],
        migration_kind: RbacMigrationKind,
    ) -> Result<(usize, Vec<String>)> {
        if migration_kind == RbacMigrationKind::Fresh {
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
    ///
    /// An `active` migration checkpoint is the durable proof that the verified
    /// transition completed. Re-scanning every Memory node during each doctor or
    /// onboarding run would repeatedly decode the entire graph in HelixDB.
    pub async fn compatibility_coverage_complete(&self) -> Result<bool> {
        let policy = self.snapshot().await?;
        if policy.migration_state == RbacMigrationState::Active
            || policy.migration_kind == Some(RbacMigrationKind::Fresh)
        {
            return Ok(true);
        }
        let ids = self.all_memory_ids().await?;
        Ok(self
            .compatibility_memory_coverage(
                &ids,
                policy.migration_kind.unwrap_or(RbacMigrationKind::Legacy),
            )
            .await?
            .1
            .is_empty())
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
            .execute_query("getAllMemoryIds", &serde_json::json!({}))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(response
            .memories
            .into_iter()
            .flatten()
            .filter(|memory_id| !memory_id.is_empty())
            .collect())
    }
}

fn principal_ready(policy: &RbacPolicy, principal: &str) -> bool {
    policy.users.get(principal).is_some_and(|binding| {
        binding.global_roles.contains(&Role::Admin)
            || [DEFAULT_GROUP_ID, ONBOARDING_GROUP_ID]
                .into_iter()
                .any(|group| {
                    binding
                        .groups
                        .get(group)
                        .is_some_and(|roles| !roles.is_empty())
                })
    })
}

#[derive(Debug, Default, Deserialize)]
struct AllMemoriesResponse {
    #[serde(default)]
    memories: Vec<Option<String>>,
}

#[cfg(test)]
#[path = "rbac_compat/tests.rs"]
mod tests;
