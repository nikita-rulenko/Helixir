//! Private HelixDB response projections used by the RBAC service.

use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct MemoryRbacNode {
    pub(super) id: String,
    pub(super) memory_id: String,
    #[serde(default)]
    pub(super) rbac_scope: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GroupRbacNode {
    pub(super) id: String,
    pub(super) group_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct DedupGroupRbacNode {
    pub(super) id: String,
    pub(super) dedup_group_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct MemoryRbacLink {
    pub(super) from_node: String,
    pub(super) to_node: String,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct MemoryRbacScopesResponse {
    #[serde(default)]
    pub(super) memories: Vec<MemoryRbacNode>,
    #[serde(default)]
    pub(super) group_links: Vec<MemoryRbacLink>,
    #[serde(default)]
    pub(super) groups: Vec<GroupRbacNode>,
    #[serde(default)]
    pub(super) dedup_links: Vec<MemoryRbacLink>,
    #[serde(default)]
    pub(super) dedup_groups: Vec<DedupGroupRbacNode>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct MemoryRbacScopeResponse {
    #[serde(default)]
    pub(super) memory: Option<MemoryRbacNode>,
    #[serde(default)]
    pub(super) group_links: Vec<MemoryRbacLink>,
    #[serde(default)]
    pub(super) groups: Vec<GroupRbacNode>,
    #[serde(default)]
    pub(super) dedup_links: Vec<MemoryRbacLink>,
    #[serde(default)]
    pub(super) dedup_groups: Vec<DedupGroupRbacNode>,
}

impl MemoryRbacScopesResponse {
    pub(super) fn append_single(&mut self, mut single: MemoryRbacScopeResponse) {
        if let Some(memory) = single.memory.take() {
            self.memories.push(memory);
        }
        self.group_links.append(&mut single.group_links);
        self.groups.append(&mut single.groups);
        self.dedup_links.append(&mut single.dedup_links);
        self.dedup_groups.append(&mut single.dedup_groups);
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct StoredMemoryScope {
    pub(super) rbac_scope: String,
    pub(super) groups: HashSet<String>,
    pub(super) dedup_groups: HashSet<String>,
}

impl StoredMemoryScope {
    #[cfg(test)]
    pub(super) fn is_legacy_unscoped(&self) -> bool {
        self.rbac_scope.is_empty() && self.groups.is_empty() && self.dedup_groups.is_empty()
    }

    pub(super) fn needs_default_workspace_migration(&self) -> bool {
        self.rbac_scope.is_empty()
            && self.dedup_groups.is_empty()
            && (self.groups.is_empty()
                || self
                    .groups
                    .contains(crate::core::rbac_compat::ONBOARDING_GROUP_ID))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct DedupGroupMemoryNode {
    #[serde(default)]
    pub(super) memory_id: String,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct DedupGroupMemoriesResponse {
    #[serde(default)]
    pub(super) memories: Vec<DedupGroupMemoryNode>,
}

pub(super) fn assignment_id(subject: &str, group: &str, role: Role) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(subject.as_bytes());
    hasher.update([0]);
    hasher.update(group.as_bytes());
    hasher.update([0]);
    hasher.update(role.label().as_bytes());
    format!("rbac_{:x}", hasher.finalize())
}

pub(super) fn reject_reserved_group_mutation(group_id: &str, action: &str) -> Result<()> {
    if matches!(
        group_id,
        crate::core::rbac_compat::DEFAULT_GROUP_ID | crate::core::rbac_compat::ONBOARDING_GROUP_ID
    ) {
        bail!("cannot {action} the reserved RBAC group '{group_id}'")
    }
    Ok(())
}

pub(super) fn ensure_admin_revoke_is_recoverable(
    policy: &RbacPolicy,
    subject_id: &str,
    role: Role,
    group_id: &str,
) -> Result<()> {
    if !policy.enabled || !group_id.is_empty() || role != Role::Admin {
        return Ok(());
    }
    let active_admins = policy
        .users
        .iter()
        .filter(|(_, binding)| binding.global_roles.contains(&Role::Admin))
        .count();
    let subject_is_admin = policy.is_admin(subject_id);
    if subject_is_admin && active_admins <= 1 {
        bail!("cannot revoke the last global admin while RBAC is enabled")
    }
    Ok(())
}

pub(super) fn rows<'a>(value: &'a serde_json::Value, key: &str) -> Vec<&'a serde_json::Value> {
    value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .or_else(|| value.as_array())
        .map(|rows| rows.iter().collect())
        .unwrap_or_default()
}

pub(super) fn apply_dedup_memberships(policy: &mut RbacPolicy, value: &serde_json::Value) {
    let groups = rows(value, "groups")
        .into_iter()
        .filter_map(|row| Some((string_field(row, "id")?, string_field(row, "group_id")?)))
        .collect::<HashMap<_, _>>();
    let dedup_groups = rows(value, "dedup_groups")
        .into_iter()
        .filter_map(|row| {
            Some((
                string_field(row, "id")?,
                string_field(row, "dedup_group_id")?,
            ))
        })
        .collect::<HashMap<_, _>>();
    let links = if value.get("dedup_links").is_some() {
        rows(value, "dedup_links")
    } else {
        rows(value, "links")
    };
    for link in links {
        let (Some(from), Some(to)) = (
            string_field(link, "from_node"),
            string_field(link, "to_node"),
        ) else {
            continue;
        };
        let (Some(group_id), Some(dedup_group_id)) = (groups.get(&from), dedup_groups.get(&to))
        else {
            continue;
        };
        if policy.dedup_groups.contains_key(dedup_group_id)
            && let Some(group) = policy.groups.get_mut(group_id)
        {
            group.dedup_group_id = Some(dedup_group_id.clone());
        }
    }
}

pub(super) fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|field| field.as_str())
        .map(str::to_owned)
}

pub(super) fn number_as_i64(value: &serde_json::Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

pub(super) fn is_missing_rbac_surface(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("couldn't find")
        || lower.contains("could not find")
        || lower.contains("no value found")
}
