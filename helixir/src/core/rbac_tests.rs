use super::*;

fn sample() -> RbacPolicy {
    let mut p = RbacPolicy {
        enabled: true,
        ..Default::default()
    };
    p.upsert_group("alpha", "Alpha team");
    p.upsert_group("beta", "Beta team");
    p.assign_global("root", Role::Admin);
    p.assign_group("lead", "alpha", Role::TeamLead).unwrap();
    p.assign_group("mod", "alpha", Role::Moderator).unwrap();
    p.assign_group("worker", "alpha", Role::Worker).unwrap();
    p.assign_group("viewer", "alpha", Role::Viewer).unwrap();
    p
}

#[test]
fn role_parser_accepts_cli_aliases() {
    assert_eq!(Role::parse("team-lead"), Some(Role::TeamLead));
    assert_eq!(Role::parse("read-only"), Some(Role::Viewer));
    assert_eq!(Role::parse("nope"), None);
}

#[test]
fn only_bare_pre_rbac_scope_is_legacy_unscoped() {
    let legacy = StoredMemoryScope::default();
    assert!(legacy.is_legacy_unscoped());

    for rbac_scope in ["rbac:unscoped", "rbac:group:alpha", "rbac:dedup:dev"] {
        assert!(
            !StoredMemoryScope {
                rbac_scope: rbac_scope.to_string(),
                ..Default::default()
            }
            .is_legacy_unscoped()
        );
    }

    let mut group_scoped = StoredMemoryScope::default();
    group_scoped.groups.insert("alpha".to_string());
    assert!(!group_scoped.is_legacy_unscoped());

    let mut dedup_scoped = StoredMemoryScope::default();
    dedup_scoped.dedup_groups.insert("development".to_string());
    assert!(!dedup_scoped.is_legacy_unscoped());
}

#[test]
fn admin_reads_everyone_and_viewer_only_group() {
    let p = sample();
    assert!(p.readable_users("root").is_none());
    let visible = p.readable_users("viewer").unwrap();
    assert!(visible.contains("worker"));
    assert!(visible.contains("viewer"));
    assert!(!visible.contains("root"));
}

#[test]
fn viewer_cannot_write_but_worker_can_write_own_memory() {
    let p = sample();
    assert!(!p.can_write("viewer"));
    assert!(p.can_write_owner("worker", "worker"));
    assert!(p.can_write_owner("mod", "worker"));
    assert!(!p.can_write_owner("worker", "mod"));
}

#[test]
fn disabled_policy_is_full_trust() {
    let p = RbacPolicy::default();
    assert!(p.can_write("unknown"));
    assert!(p.readable_users("unknown").is_none());
    assert!(p.can_read_pending("unknown", "owner", "creator"));
    assert!(p.can_read_outbox("unknown", "owner"));
}

#[test]
fn pending_and_outbox_payloads_are_not_group_readable() {
    let p = sample();
    assert!(p.can_read_pending("root", "worker", "mod"));
    assert!(p.can_read_pending("worker", "worker", "mod"));
    assert!(p.can_read_pending("mod", "worker", "mod"));
    assert!(!p.can_read_pending("viewer", "worker", "mod"));
    assert!(!p.can_read_pending("lead", "worker", "mod"));

    assert!(p.can_read_outbox("root", "worker"));
    assert!(p.can_read_outbox("worker", "worker"));
    assert!(!p.can_read_outbox("mod", "worker"));
    assert!(!p.can_read_outbox("viewer", "worker"));
}

#[test]
fn deny_by_default_for_unassigned_principal() {
    let p = sample();
    assert!(p.readable_users("unassigned").unwrap().is_empty());
    assert!(!p.can_write("unassigned"));
}

#[test]
fn every_role_has_expected_write_semantics() {
    let mut p = RbacPolicy {
        enabled: true,
        ..Default::default()
    };
    p.upsert_group("g", "Group");
    for (user, role) in [
        ("admin", Role::Admin),
        ("lead", Role::TeamLead),
        ("group-admin", Role::GroupAdmin),
        ("moderator", Role::Moderator),
        ("worker", Role::Worker),
        ("viewer", Role::Viewer),
    ] {
        if role == Role::Admin {
            p.assign_global(user, role);
        } else {
            p.assign_group(user, "g", role).unwrap();
        }
    }
    assert!(p.can_write("admin"));
    assert!(!p.can_write("lead"));
    assert!(p.can_write("group-admin"));
    assert!(p.can_write("moderator"));
    assert!(p.can_write("worker"));
    assert!(!p.can_write("viewer"));
}

#[test]
fn groups_isolate_readers() {
    let mut p = RbacPolicy {
        enabled: true,
        ..Default::default()
    };
    p.upsert_group("a", "A");
    p.upsert_group("b", "B");
    p.assign_group("alice", "a", Role::Worker).unwrap();
    p.assign_group("bob", "b", Role::Worker).unwrap();
    p.assign_group("auditor", "a", Role::Viewer).unwrap();
    let visible = p.readable_users("auditor").unwrap();
    assert!(visible.contains("alice"));
    assert!(!visible.contains("bob"));
}

#[test]
fn actor_and_owner_are_checked_separately_for_new_memories() {
    let mut p = sample();
    p.assign_group("group-admin", "alpha", Role::GroupAdmin)
        .unwrap();
    assert!(p.can_create_for("root", "worker"));
    assert!(p.can_create_for("mod", "worker"));
    assert!(p.can_create_for("worker", "worker"));
    assert!(!p.can_create_for("worker", "mod"));
    assert!(!p.can_create_for("viewer", "worker"));
    assert!(p.can_create_for("group-admin", "worker"));
}

#[test]
fn multi_group_owner_requires_explicit_write_group() {
    let mut p = sample();
    p.assign_group("worker", "beta", Role::Worker).unwrap();
    p.assign_group("beta-viewer", "beta", Role::Viewer).unwrap();

    assert!(p.can_create_for_group("worker", "worker", Some("alpha")));
    assert!(p.can_create_for_group("worker", "worker", Some("beta")));
    assert!(!p.can_create_for_group("worker", "worker", None));
    assert!(!p.can_create_for_group("viewer", "viewer", Some("alpha")));

    let alpha_only = HashSet::from(["alpha".to_string()]);
    assert!(p.can_write_memory("worker", "worker", &alpha_only));
    assert!(!p.can_write_memory("beta-viewer", "worker", &alpha_only));
}

#[test]
fn unscoped_enabled_memory_is_admin_only() {
    let p = sample();
    let unscoped = HashSet::new();
    assert!(p.can_write_memory("root", "worker", &unscoped));
    assert!(!p.can_write_memory("worker", "worker", &unscoped));
    assert!(p.can_create_for_group("root", "worker", None));
    assert!(!p.can_create_for_group("worker", "worker", None));
}

#[test]
fn dedup_federation_shares_one_scope_across_current_groups() {
    let mut p = sample();
    p.upsert_dedup_group("development", "Engineering knowledge");
    p.assign_dedup_group("alpha", Some("development")).unwrap();
    p.assign_dedup_group("beta", Some("development")).unwrap();

    let alpha = p.resolve_memory_scope(Some("alpha")).unwrap();
    let beta = p.resolve_memory_scope(Some("beta")).unwrap();
    assert_eq!(alpha.fingerprint_scope(), beta.fingerprint_scope());
    assert_eq!(
        alpha.group_ids(),
        BTreeSet::from(["alpha".to_string(), "beta".to_string()])
    );
}

#[test]
fn leaving_federation_isolates_new_writes_without_erasing_history() {
    let mut p = sample();
    p.upsert_dedup_group("development", "Engineering knowledge");
    p.assign_dedup_group("alpha", Some("development")).unwrap();
    p.assign_dedup_group("beta", Some("development")).unwrap();
    let historical = p.resolve_memory_scope(Some("beta")).unwrap();

    p.assign_dedup_group("beta", None).unwrap();
    let future = p.resolve_memory_scope(Some("beta")).unwrap();
    assert_ne!(historical.fingerprint_scope(), future.fingerprint_scope());
    assert_eq!(future.group_ids(), BTreeSet::from(["beta".to_string()]));
    assert_eq!(
        p.resolve_memory_scope(Some("alpha")).unwrap().group_ids(),
        BTreeSet::from(["alpha".to_string()])
    );
}

#[test]
fn only_missing_schema_errors_preserve_legacy_disabled_mode() {
    assert!(is_missing_rbac_surface(
        "Couldn't find setRbacEnabled of type Query (NOT_FOUND)"
    ));
    assert!(is_missing_rbac_surface("Graph error: No value found"));
    assert!(!is_missing_rbac_surface("Connection failed: timeout"));
    assert!(!is_missing_rbac_surface("permission denied"));
}

#[test]
fn onboarding_group_cannot_be_removed_or_federated() {
    assert!(reject_reserved_group_mutation("onboarding", "deactivate").is_err());
    assert!(reject_reserved_group_mutation("onboarding", "attach to a dedup federation").is_err());
    assert!(reject_reserved_group_mutation("development", "deactivate").is_ok());
}

#[test]
fn enabled_policy_cannot_revoke_its_last_global_admin() {
    let mut policy = RbacPolicy {
        enabled: true,
        ..Default::default()
    };
    policy.assign_global("root", Role::Admin);
    assert!(ensure_admin_revoke_is_recoverable(&policy, "root", Role::Admin, "").is_err());

    policy.assign_global("backup", Role::Admin);
    assert!(ensure_admin_revoke_is_recoverable(&policy, "root", Role::Admin, "").is_ok());
}
