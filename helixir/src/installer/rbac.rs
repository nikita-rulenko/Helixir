//! Typed installer choices for the graph-backed RBAC profile.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::ClientKind;
use crate::core::{ONBOARDING_GROUP_ID, RbacManager, Role};

/// Security profile selected by onboarding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RbacInstallOptions {
    /// Enable graph-backed enforcement after bootstrap.
    pub enabled: bool,
    /// Sole initial global administrator.
    pub operator_id: String,
    /// MCP principals enrolled as group administrators in `onboarding`.
    pub principals: BTreeSet<String>,
}

/// Read-only graph state used to keep repeat onboarding idempotent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RbacInstallState {
    pub enabled: bool,
    pub compatibility_group_exists: bool,
    pub global_admins: BTreeSet<String>,
    pub compatibility_group_admins: BTreeSet<String>,
    /// Whether the onboarding registry is initialized. Memory-provenance User
    /// nodes created after bootstrap are not principals until explicitly admitted.
    pub all_users_enrolled: bool,
    pub all_memories_covered: bool,
}

impl RbacInstallState {
    /// Whether the live graph already satisfies the requested profile.
    #[must_use]
    pub fn satisfies(&self, options: &RbacInstallOptions) -> bool {
        if !options.enabled {
            return !self.enabled;
        }
        self.enabled
            && self.compatibility_group_exists
            && self.global_admins.contains(&options.operator_id)
            && options
                .principals
                .iter()
                .chain(std::iter::once(&options.operator_id))
                .all(|principal| self.compatibility_group_admins.contains(principal))
            && self.all_users_enrolled
            && self.all_memories_covered
    }
}

/// Read the onboarding profile from HelixDB without mutating it.
pub async fn inspect(manager: &RbacManager) -> anyhow::Result<RbacInstallState> {
    let policy = manager.snapshot().await?;
    Ok(RbacInstallState {
        enabled: policy.enabled,
        compatibility_group_exists: policy.groups.contains_key(ONBOARDING_GROUP_ID),
        global_admins: policy
            .users
            .iter()
            .filter(|(_, binding)| binding.global_roles.contains(&Role::Admin))
            .map(|(id, _)| id.clone())
            .collect(),
        compatibility_group_admins: policy
            .users
            .iter()
            .filter(|(_, binding)| {
                binding
                    .groups
                    .get(ONBOARDING_GROUP_ID)
                    .is_some_and(|roles| roles.contains(&Role::GroupAdmin))
            })
            .map(|(id, _)| id.clone())
            .collect(),
        all_users_enrolled: manager.compatibility_user_coverage_complete().await?,
        all_memories_covered: manager.compatibility_coverage_complete().await?,
    })
}

impl Default for RbacInstallOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            operator_id: "helixir-operator".to_string(),
            principals: BTreeSet::new(),
        }
    }
}

/// Durable RBAC selection written to the install manifest.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RbacManifest {
    pub enabled: bool,
    #[serde(default)]
    pub operator_id: String,
    #[serde(default)]
    pub group_id: String,
    #[serde(default)]
    pub principals: Vec<String>,
}

impl ClientKind {
    /// Stable authenticated principal written into each supported MCP client.
    #[must_use]
    pub const fn principal_id(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_state() -> RbacInstallState {
        RbacInstallState {
            enabled: true,
            compatibility_group_exists: true,
            global_admins: BTreeSet::from(["root".to_string()]),
            compatibility_group_admins: BTreeSet::from(["root".to_string(), "codex".to_string()]),
            all_users_enrolled: true,
            all_memories_covered: true,
        }
    }

    #[test]
    fn profile_requires_user_and_memory_coverage() {
        let options = RbacInstallOptions {
            enabled: true,
            operator_id: "root".to_string(),
            principals: BTreeSet::from(["codex".to_string()]),
        };
        assert!(ready_state().satisfies(&options));

        let mut missing_user = ready_state();
        missing_user.all_users_enrolled = false;
        assert!(!missing_user.satisfies(&options));

        let mut missing_memory = ready_state();
        missing_memory.all_memories_covered = false;
        assert!(!missing_memory.satisfies(&options));
    }
}
