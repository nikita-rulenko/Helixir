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
fn memory_id_projection_deserializes_scalars_and_nulls() {
    let response: AllMemoriesResponse = serde_json::from_value(serde_json::json!({
        "memories": ["mem_one", null, "", "mem_two"]
    }))
    .unwrap();
    let ids = response
        .memories
        .into_iter()
        .flatten()
        .filter(|id| !id.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["mem_one", "mem_two"]);
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
