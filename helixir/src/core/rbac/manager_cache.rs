//! Revision-checked process cache for graph-backed RBAC policy snapshots.

use super::*;
use std::sync::{Mutex as StdMutex, OnceLock};
use tokio::sync::{Mutex, RwLock};

#[derive(Clone)]
struct CachedPolicy {
    revision: PolicyRevision,
    policy: RbacPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PolicyRevision {
    enabled: i64,
    migration_state: String,
    migration_kind: String,
    updated_at: String,
    updated_by: String,
}

pub(super) struct PolicyCache {
    current: RwLock<Option<CachedPolicy>>,
    refresh: Mutex<()>,
}

impl PolicyCache {
    fn new() -> Self {
        Self {
            current: RwLock::new(None),
            refresh: Mutex::new(()),
        }
    }
}

pub(super) fn policy_cache_for(db: &HelixClient) -> Arc<PolicyCache> {
    static CACHES: OnceLock<StdMutex<HashMap<String, Arc<PolicyCache>>>> = OnceLock::new();
    let caches = CACHES.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut caches = caches
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Arc::clone(
        caches
            .entry(db.base_url().to_string())
            .or_insert_with(|| Arc::new(PolicyCache::new())),
    )
}

impl RbacManager {
    /// Load the current graph policy, reusing a snapshot only while the
    /// graph-backed revision is unchanged. Every authorization still reads the
    /// single `RbacConfig` node, so revocations are observed without a TTL.
    pub async fn snapshot(&self) -> Result<RbacPolicy> {
        let mut config = self.read_config().await?;
        let mut revision = policy_revision(&config);
        if let Some(policy) = self.cached_policy(&revision).await {
            return Ok(policy);
        }

        let _refresh = self.cache.refresh.lock().await;
        config = self.read_config().await?;
        revision = policy_revision(&config);
        if let Some(policy) = self.cached_policy(&revision).await {
            return Ok(policy);
        }

        match self.read_atomic_snapshot().await {
            Ok(snapshot) => {
                let loaded_revision = policy_revision(&snapshot);
                let policy = parse_policy(&snapshot, &snapshot, &snapshot, &snapshot, &snapshot)?;
                *self.cache.current.write().await = Some(CachedPolicy {
                    revision: loaded_revision,
                    policy: policy.clone(),
                });
                Ok(policy)
            }
            Err(error) if is_missing_rbac_surface(&error.to_string()) => {
                // Rolling compatibility for a backend that predates the atomic
                // snapshot query. It is deliberately not cached because those
                // old mutation queries do not bump the policy revision.
                self.read_legacy_snapshot(&config).await
            }
            Err(error) => Err(error),
        }
    }

    async fn cached_policy(&self, revision: &PolicyRevision) -> Option<RbacPolicy> {
        self.cache
            .current
            .read()
            .await
            .as_ref()
            .filter(|cached| &cached.revision == revision)
            .map(|cached| cached.policy.clone())
    }

    async fn read_config(&self) -> Result<serde_json::Value> {
        match self
            .db
            .execute_query("getRbacConfig", &serde_json::json!({}))
            .await
        {
            Ok(value) => Ok(value),
            Err(error) if is_missing_rbac_surface(&error.to_string()) => {
                Ok(serde_json::Value::Null)
            }
            Err(error) => Err(anyhow::anyhow!(error.to_string())),
        }
    }

    async fn read_atomic_snapshot(&self) -> Result<serde_json::Value> {
        self.db
            .execute_query("getRbacPolicySnapshot", &serde_json::json!({}))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }

    async fn read_legacy_snapshot(&self, config: &serde_json::Value) -> Result<RbacPolicy> {
        let enabled = config
            .get("config")
            .and_then(|row| row.get("enabled"))
            .and_then(number_as_i64)
            .unwrap_or(0)
            != 0;
        let groups = match self
            .db
            .execute_query("getRbacGroups", &serde_json::json!({}))
            .await
        {
            Ok(value) => value,
            Err(error) if !enabled && is_missing_rbac_surface(&error.to_string()) => {
                return Ok(RbacPolicy::default());
            }
            Err(error) => return Err(anyhow::anyhow!(error.to_string())),
        };
        let assignments = self
            .db
            .execute_query("getRbacAssignments", &serde_json::json!({}))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let dedup_groups = match self
            .db
            .execute_query("getRbacDedupGroups", &serde_json::json!({}))
            .await
        {
            Ok(value) => value,
            Err(error) if !enabled && is_missing_rbac_surface(&error.to_string()) => {
                serde_json::json!({"dedup_groups": []})
            }
            Err(error) => return Err(anyhow::anyhow!(error.to_string())),
        };
        let memberships = match self
            .db
            .execute_query("getRbacDedupMemberships", &serde_json::json!({}))
            .await
        {
            Ok(value) => value,
            Err(error) if !enabled && is_missing_rbac_surface(&error.to_string()) => {
                serde_json::json!({"groups": [], "links": [], "dedup_groups": []})
            }
            Err(error) => return Err(anyhow::anyhow!(error.to_string())),
        };
        parse_policy(config, &groups, &assignments, &dedup_groups, &memberships)
    }
}

fn policy_revision(value: &serde_json::Value) -> PolicyRevision {
    let config = value.get("config");
    PolicyRevision {
        enabled: config
            .and_then(|row| row.get("enabled"))
            .and_then(number_as_i64)
            .unwrap_or(0),
        migration_state: config
            .and_then(|row| row.get("migration_state"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        migration_kind: config
            .and_then(|row| row.get("migration_kind"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        updated_at: config
            .and_then(|row| row.get("updated_at"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        updated_by: config
            .and_then(|row| row.get("updated_by"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }
}

fn parse_policy(
    config_value: &serde_json::Value,
    groups_value: &serde_json::Value,
    assignments_value: &serde_json::Value,
    dedup_groups_value: &serde_json::Value,
    dedup_memberships_value: &serde_json::Value,
) -> Result<RbacPolicy> {
    let config = config_value.get("config");
    let mut policy = RbacPolicy {
        enabled: config
            .and_then(|row| row.get("enabled"))
            .and_then(number_as_i64)
            .unwrap_or(0)
            != 0,
        migration_state: config
            .and_then(|row| row.get("migration_state"))
            .and_then(serde_json::Value::as_str)
            .map(RbacMigrationState::parse)
            .unwrap_or_default(),
        migration_kind: config
            .and_then(|row| row.get("migration_kind"))
            .and_then(serde_json::Value::as_str)
            .and_then(RbacMigrationKind::parse),
        ..Default::default()
    };
    for row in rows(groups_value, "groups") {
        let Some(group_id) = string_field(row, "group_id") else {
            continue;
        };
        policy.groups.insert(
            group_id.clone(),
            Group {
                name: string_field(row, "name").unwrap_or_else(|| group_id.clone()),
                description: string_field(row, "description").unwrap_or_default(),
                dedup_group_id: None,
            },
        );
    }
    for row in rows(dedup_groups_value, "dedup_groups") {
        let Some(dedup_group_id) = string_field(row, "dedup_group_id") else {
            continue;
        };
        policy.dedup_groups.insert(
            dedup_group_id.clone(),
            DedupGroup {
                name: string_field(row, "name").unwrap_or_else(|| dedup_group_id.clone()),
                description: string_field(row, "description").unwrap_or_default(),
            },
        );
    }
    apply_dedup_memberships(&mut policy, dedup_memberships_value);
    for row in rows(assignments_value, "assignments") {
        let (Some(subject), Some(role_name)) =
            (string_field(row, "subject_id"), string_field(row, "role"))
        else {
            continue;
        };
        let Some(role) = Role::parse(&role_name) else {
            continue;
        };
        let group = string_field(row, "group_id").unwrap_or_default();
        if group.is_empty() {
            policy.assign_global(&subject, role);
        } else if policy.groups.contains_key(&group) {
            let _ = policy.assign_group(&subject, &group, role);
        }
    }
    policy.validate()?;
    Ok(policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_changes_on_every_security_relevant_config_field() {
        let first = serde_json::json!({"config": {
            "enabled": 1, "migration_state": "active", "migration_kind": "fresh",
            "updated_at": "one", "updated_by": "admin"
        }});
        let mut second = first.clone();
        second["config"]["updated_at"] = serde_json::json!("two");
        assert_ne!(policy_revision(&first), policy_revision(&second));
    }

    #[test]
    fn combined_snapshot_projects_assignments_and_dedup_membership() {
        let value = serde_json::json!({
            "config": {"enabled": 1, "migration_state": "active", "migration_kind": "fresh"},
            "groups": [{"id": "g-node", "group_id": "g", "name": "G"}],
            "assignments": [{"subject_id": "worker", "role": "worker", "group_id": "g"}],
            "dedup_groups": [{"id": "d-node", "dedup_group_id": "d", "name": "D"}],
            "dedup_links": [{"from_node": "g-node", "to_node": "d-node"}]
        });
        let policy = parse_policy(&value, &value, &value, &value, &value).unwrap();
        assert!(policy.can_write("worker"));
        assert_eq!(policy.groups["g"].dedup_group_id.as_deref(), Some("d"));
    }
}
