//! Typed installer choices for the graph-backed RBAC profile.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::ClientKind;
use crate::core::{
    DEFAULT_GROUP_ID, MOIRAI_GROUP_ID, ONBOARDING_GROUP_ID, RbacManager, RbacMigrationState, Role,
};

/// Security profile selected by onboarding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RbacInstallOptions {
    /// Sole initial global administrator.
    pub operator_id: String,
    /// MCP principals admitted to `onboarding` on fresh installs or `default`
    /// during a legacy upgrade.
    pub principals: BTreeSet<String>,
}

/// Read-only graph state used to keep repeat onboarding idempotent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RbacInstallState {
    pub enabled: bool,
    pub migration_active: bool,
    pub default_group_exists: bool,
    pub onboarding_group_exists: bool,
    pub moirai_group_exists: bool,
    pub global_admins: BTreeSet<String>,
    pub registered_principals: BTreeSet<String>,
    pub all_users_registered: bool,
    pub legacy_memories_covered: bool,
}

impl RbacInstallState {
    /// Whether the live graph already satisfies the requested profile.
    #[must_use]
    pub fn satisfies(&self, options: &RbacInstallOptions) -> bool {
        self.enabled
            && self.migration_active
            && self.default_group_exists
            && self.onboarding_group_exists
            && self.moirai_group_exists
            && self.global_admins.contains(&options.operator_id)
            && options
                .principals
                .iter()
                .chain(std::iter::once(&options.operator_id))
                .all(|principal| self.registered_principals.contains(principal))
            && self.all_users_registered
            && self.legacy_memories_covered
    }
}

/// Read the onboarding profile from HelixDB without mutating it.
pub async fn inspect(manager: &RbacManager) -> anyhow::Result<RbacInstallState> {
    let policy = manager.snapshot().await?;
    Ok(RbacInstallState {
        enabled: policy.enabled,
        migration_active: policy.migration_state == RbacMigrationState::Active,
        default_group_exists: policy.groups.contains_key(DEFAULT_GROUP_ID),
        onboarding_group_exists: policy.groups.contains_key(ONBOARDING_GROUP_ID),
        moirai_group_exists: policy.groups.contains_key(MOIRAI_GROUP_ID),
        global_admins: policy
            .users
            .iter()
            .filter(|(_, binding)| binding.global_roles.contains(&Role::Admin))
            .map(|(id, _)| id.clone())
            .collect(),
        registered_principals: policy
            .users
            .iter()
            .filter(|(_, binding)| {
                [DEFAULT_GROUP_ID, ONBOARDING_GROUP_ID]
                    .into_iter()
                    .any(|group| {
                        binding
                            .groups
                            .get(group)
                            .is_some_and(|roles| !roles.is_empty())
                    })
            })
            .map(|(id, _)| id.clone())
            .collect(),
        all_users_registered: manager.compatibility_user_coverage_complete().await?,
        legacy_memories_covered: manager.compatibility_coverage_complete().await?,
    })
}

impl Default for RbacInstallOptions {
    fn default() -> Self {
        Self {
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
            migration_active: true,
            default_group_exists: true,
            onboarding_group_exists: true,
            moirai_group_exists: true,
            global_admins: BTreeSet::from(["root".to_string()]),
            registered_principals: BTreeSet::from(["root".to_string(), "codex".to_string()]),
            all_users_registered: true,
            legacy_memories_covered: true,
        }
    }

    #[test]
    fn profile_requires_user_and_memory_coverage() {
        let options = RbacInstallOptions {
            operator_id: "root".to_string(),
            principals: BTreeSet::from(["codex".to_string()]),
        };
        assert!(ready_state().satisfies(&options));

        let mut missing_user = ready_state();
        missing_user.all_users_registered = false;
        assert!(!missing_user.satisfies(&options));

        let mut missing_memory = ready_state();
        missing_memory.legacy_memories_covered = false;
        assert!(!missing_memory.satisfies(&options));

        let mut missing_moirai = ready_state();
        missing_moirai.moirai_group_exists = false;
        assert!(!missing_moirai.satisfies(&options));
    }
}
