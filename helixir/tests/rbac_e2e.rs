//! HelixDB-backed RBAC contract test.
//!
//! Run explicitly with `HELIX_E2E=1 cargo test --test rbac_e2e -- --ignored`.
//! The test uses unique ids and disables enforcement on exit so it is safe to
//! run against a shared development instance.

use helixir::core::HelixirClient;

fn token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 and a deployed HelixDB RBAC schema"]
async fn rbac_graph_grants_revoke_and_deny() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");
    let rbac = client.rbac();
    let suffix = token();
    let group = format!("rbac_e2e_{suffix}");
    let admin = format!("admin_{suffix}");
    let worker = format!("worker_{suffix}");
    let viewer = format!("viewer_{suffix}");

    // Start from trusted mode. If a previous run left RBAC enabled, this
    // cleanup call intentionally fails closed unless its admin is known.
    rbac.set_enabled(false, "rbac_e2e").await.expect("disable");
    rbac.create_group(&group, &group, "RBAC E2E group")
        .await
        .expect("create group");
    rbac.grant(&admin, helixir::core::Role::Admin, None, "rbac_e2e")
        .await
        .expect("grant admin");
    rbac.grant(
        &worker,
        helixir::core::Role::Worker,
        Some(&group),
        "rbac_e2e",
    )
    .await
    .expect("grant worker");
    rbac.grant(
        &viewer,
        helixir::core::Role::Viewer,
        Some(&group),
        "rbac_e2e",
    )
    .await
    .expect("grant viewer");

    rbac.set_enabled(true, &admin).await.expect("enable");
    rbac.authorize_write(&worker)
        .await
        .expect("worker can write");
    assert!(
        rbac.authorize_write(&viewer).await.is_err(),
        "viewer must be read-only"
    );

    rbac.revoke_as(&worker, helixir::core::Role::Worker, Some(&group), &admin)
        .await
        .expect("revoke worker");
    assert!(
        rbac.authorize_write(&worker).await.is_err(),
        "revocation must take effect from HelixDB"
    );

    rbac.set_enabled(false, &admin)
        .await
        .expect("disable cleanup");
}
