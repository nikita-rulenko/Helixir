//! Live revision-cache contract for graph-backed RBAC policy snapshots.

use helixir::core::{HelixirClient, Role};

mod common;

async fn policy_revision(client: &HelixirClient, admin: &str) -> String {
    let value: serde_json::Value = client
        .admin_as(admin)
        .await
        .expect("admin surface")
        .db()
        .execute_query("getRbacConfig", &serde_json::json!({}))
        .await
        .expect("RBAC config");
    value["config"]["updated_at"]
        .as_str()
        .expect("policy revision timestamp")
        .to_string()
}

#[tokio::test]
#[ignore = "needs HELIX_E2E=1, enabled RBAC, and the revisioned policy query"]
async fn cached_policy_observes_grants_and_revocations_immediately() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");
    let admin = common::e2e_actor();
    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");
    let rbac = client.rbac();
    let initial = rbac.snapshot().await.expect("initial policy");
    assert!(initial.enabled && initial.is_admin(&admin));

    for _ in 0..1_000 {
        assert!(
            rbac.snapshot()
                .await
                .expect("cached snapshot")
                .is_admin(&admin)
        );
    }
    let revision_before = policy_revision(&client, &admin).await;

    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let group = format!("policy_cache_{suffix}");
    let viewer = format!("policy_cache_viewer_{suffix}");
    common::with_cleanup(
        async {
            rbac.create_group_as(&group, &group, "policy cache E2E", &admin)
                .await
                .expect("create group");
            rbac.grant(&viewer, Role::Worker, Some("onboarding"), &admin)
                .await
                .expect("enroll viewer");
            rbac.grant(&viewer, Role::Viewer, Some(&group), &admin)
                .await
                .expect("grant viewer");
            let revision_after = policy_revision(&client, &admin).await;
            assert_ne!(
                revision_before, revision_after,
                "the policy mutation and revision bump must commit together"
            );

            let granted = rbac.snapshot().await.expect("snapshot after grant");
            assert!(
                granted.users[&viewer].groups[&group].contains(&Role::Viewer),
                "a committed grant must invalidate the cached policy"
            );
            rbac.revoke_as(&viewer, Role::Viewer, Some(&group), &admin)
                .await
                .expect("revoke viewer");
            let revoked = rbac.snapshot().await.expect("snapshot after revoke");
            assert!(
                revoked
                    .users
                    .get(&viewer)
                    .and_then(|binding| binding.groups.get(&group))
                    .is_none_or(|roles| !roles.contains(&Role::Viewer)),
                "a committed revocation must be visible on the next authorization"
            );
        },
        async {
            let mut errors = Vec::new();
            if let Err(error) = rbac
                .revoke_as(&viewer, Role::Viewer, Some(&group), &admin)
                .await
            {
                errors.push(format!("revoke viewer: {error}"));
            }
            if let Err(error) = rbac
                .revoke_as(&viewer, Role::Worker, Some("onboarding"), &admin)
                .await
            {
                errors.push(format!("offboard viewer: {error}"));
            }
            if let Err(error) = rbac.deactivate_group_as(&group, &admin).await {
                errors.push(format!("deactivate group: {error}"));
            }
            errors
        },
    )
    .await;
}
