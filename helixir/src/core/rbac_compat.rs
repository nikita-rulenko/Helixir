//! Compatibility bootstrap for making RBAC the safe default.
//!
//! The reserved group recreates the historical shared data plane while keeping
//! the RBAC control plane restricted to one explicit operator.

use std::collections::BTreeSet;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use super::rbac::{RbacManager, RbacMemoryScope, RbacPolicy, Role};

/// Reserved group used by fresh installs and trusted-network upgrades.
pub const ONBOARDING_GROUP_ID: &str = "onboarding";
const DEFAULT_COMPATIBILITY_GROUP_NAME: &str = "Onboarding";
const DEFAULT_COMPATIBILITY_GROUP_DESCRIPTION: &str =
    "Compatibility group for the shared trusted-network memory plane";
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
    pub principals_enrolled: Vec<String>,
    pub users_registered: usize,
    pub memories_seen: usize,
    pub memories_attached: usize,
}

impl RbacPolicy {
    /// Resolve an omitted write group to the reserved compatibility group only
    /// when the actor has a write-capable role there. Explicit groups always win.
    pub fn effective_write_group(&self, actor: &str, requested: Option<&str>) -> Option<String> {
        if let Some(group_id) = requested {
            return Some(group_id.to_string());
        }
        if !self.enabled {
            return None;
        }
        self.users
            .get(actor)
            .and_then(|binding| binding.groups.get(ONBOARDING_GROUP_ID))
            .is_some_and(|roles| {
                roles.iter().any(|role| {
                    matches!(
                        role,
                        Role::Admin | Role::GroupAdmin | Role::Moderator | Role::Worker
                    )
                })
            })
            .then(|| ONBOARDING_GROUP_ID.to_string())
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

    /// Create the compatibility profile, attach every existing memory, then
    /// enable enforcement. Retrying is safe because all nodes and edges upsert.
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
        let verified_profile_replay = initial.enabled
            && initial
                .groups
                .get(ONBOARDING_GROUP_ID)
                .is_some_and(|group| {
                    group.name == DEFAULT_COMPATIBILITY_GROUP_NAME
                        && group.description == DEFAULT_COMPATIBILITY_GROUP_DESCRIPTION
                });

        let mut enrolled = principals
            .iter()
            .map(|principal| principal.trim())
            .filter(|principal| !principal.is_empty())
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        enrolled.insert(operator_id.to_string());

        self.create_group_as(
            ONBOARDING_GROUP_ID,
            DEFAULT_COMPATIBILITY_GROUP_NAME,
            DEFAULT_COMPATIBILITY_GROUP_DESCRIPTION,
            operator_id,
        )
        .await?;
        self.grant(operator_id, Role::Admin, None, operator_id)
            .await?;
        let existing_users = self.all_user_ids().await?;
        let registered_users = self.onboarding_registered_user_ids().await?;
        if !initial.enabled {
            for user_id in &existing_users {
                if !enrolled.contains(user_id) && !registered_users.contains(user_id) {
                    self.grant(
                        user_id,
                        Role::Worker,
                        Some(ONBOARDING_GROUP_ID),
                        operator_id,
                    )
                    .await?;
                }
            }
        }
        for principal in &enrolled {
            self.grant(
                principal,
                Role::GroupAdmin,
                Some(ONBOARDING_GROUP_ID),
                operator_id,
            )
            .await?;
        }

        let first_pass = if verified_profile_replay {
            0
        } else {
            self.attach_all_memories_to_compatibility(operator_id)
                .await?
        };
        let enabled_by_bootstrap = !initial.enabled;
        if enabled_by_bootstrap {
            self.set_enabled(true, operator_id).await?;
        }

        let result = async {
            let second_pass = if verified_profile_replay {
                0
            } else {
                self.attach_all_memories_to_compatibility(operator_id)
                    .await?
            };
            let (memories_seen, missing) = if verified_profile_replay {
                (0, Vec::new())
            } else {
                self.compatibility_memory_coverage().await?
            };
            if !missing.is_empty() {
                bail!(
                    "RBAC bootstrap verification failed: {} memories lack the onboarding-group edge",
                    missing.len()
                );
            }
            let policy = self.snapshot().await?;
            if !policy.enabled
                || !policy.is_admin(operator_id)
                || !policy.groups.contains_key(ONBOARDING_GROUP_ID)
            {
                bail!("RBAC bootstrap verification failed: policy graph is incomplete");
            }
            for principal in &enrolled {
                let ready = policy
                    .users
                    .get(principal)
                    .and_then(|binding| binding.groups.get(ONBOARDING_GROUP_ID))
                    .is_some_and(|roles| roles.contains(&Role::GroupAdmin));
                if !ready {
                    bail!("RBAC bootstrap verification failed for principal '{principal}'");
                }
            }
            if !initial.enabled {
                let registered_users = self.onboarding_registered_user_ids().await?;
                if existing_users
                    .iter()
                    .any(|user_id| !registered_users.contains(user_id))
                {
                    bail!(
                        "RBAC bootstrap verification failed: user registry coverage is incomplete"
                    );
                }
            }
            Ok(CompatibilityBootstrapReport {
                enabled_before: initial.enabled,
                enabled_after: true,
                operator_id: operator_id.to_string(),
                group_id: ONBOARDING_GROUP_ID.to_string(),
                principals_enrolled: enrolled.into_iter().collect(),
                users_registered: existing_users.len(),
                memories_seen,
                memories_attached: first_pass.saturating_add(second_pass),
            })
        }
        .await;

        if result.is_err() && enabled_by_bootstrap {
            let _ = self.set_enabled(false, operator_id).await;
        }
        result
    }

    async fn attach_all_memories_to_compatibility(&self, actor: &str) -> Result<usize> {
        let ids = self.all_memory_ids().await?;
        let mut attached = 0usize;
        for batch in ids.chunks(VERIFY_BATCH_SIZE) {
            let legacy_ids = self.legacy_unscoped_memory_ids(batch).await?;
            for memory_id in legacy_ids {
                self.link_memory_to_group(&memory_id, Some(ONBOARDING_GROUP_ID), actor)
                    .await?;
                attached += 1;
            }
        }
        Ok(attached)
    }

    async fn compatibility_memory_coverage(&self) -> Result<(usize, Vec<String>)> {
        let ids = self.all_memory_ids().await?;
        let mut missing = Vec::new();
        for batch in ids.chunks(VERIFY_BATCH_SIZE) {
            missing.extend(self.legacy_unscoped_memory_ids(batch).await?);
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
        let registered = self.onboarding_registered_user_ids().await?;
        Ok(policy.groups.contains_key(ONBOARDING_GROUP_ID) && !registered.is_empty())
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
            ONBOARDING_GROUP_ID.to_string(),
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
                groups: [(
                    ONBOARDING_GROUP_ID.to_string(),
                    BTreeSet::from([Role::GroupAdmin]),
                )]
                .into_iter()
                .collect(),
            },
        );
        policy
    }

    #[test]
    fn compatibility_group_keeps_legacy_fingerprint_and_adds_visibility() {
        let policy = compatibility_policy();
        let scope = policy
            .resolve_memory_scope(Some(ONBOARDING_GROUP_ID))
            .unwrap();
        assert_eq!(scope.fingerprint_scope(), None);
        assert_eq!(
            scope.group_ids(),
            BTreeSet::from([ONBOARDING_GROUP_ID.to_string()])
        );
    }

    #[test]
    fn omitted_group_is_inferred_only_for_enrolled_writer() {
        let policy = compatibility_policy();
        assert_eq!(
            policy.effective_write_group("agent", None).as_deref(),
            Some(ONBOARDING_GROUP_ID)
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
        assert!(policy.can_create_for_group("agent", "legacy-owner", Some(ONBOARDING_GROUP_ID)));
        assert!(policy.can_write_memory(
            "agent",
            "legacy-owner",
            &[ONBOARDING_GROUP_ID.to_string()].into_iter().collect()
        ));
    }
}
