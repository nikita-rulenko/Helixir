//! Enabled-state E2E for RBAC on secondary/bearer-like surfaces (#118).

mod common;

use common::McpClient;
use helixir::core::{HelixirClient, Role};
use serde_json::json;

#[tokio::test]
#[ignore = "needs HELIX_E2E=1, enabled RBAC, deployed schema, and helixir-mcp"]
async fn secondary_surfaces_are_actor_bound_and_fail_closed() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let admin = std::env::var("HELIXIR_RBAC_ACTOR").unwrap_or_else(|_| "codex".to_string());
    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");
    let rbac = client.rbac();
    let initial = rbac.snapshot().await.expect("RBAC snapshot");
    assert!(initial.enabled, "E2E requires RBAC enabled");
    assert!(initial.is_admin(&admin), "HELIXIR_RBAC_ACTOR must be admin");

    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let group = format!("rbac_secondary_{suffix}");
    let worker = format!("secondary_worker_{suffix}");
    let viewer = format!("secondary_viewer_{suffix}");

    rbac.create_group_as(&group, &group, "secondary RBAC E2E", &admin)
        .await
        .expect("create group");
    rbac.grant(&worker, Role::Worker, Some(&group), &admin)
        .await
        .expect("grant worker");
    rbac.grant(&viewer, Role::Viewer, Some(&group), &admin)
        .await
        .expect("grant viewer");

    assert!(client.admin_as(&viewer).await.is_err());
    assert!(client.admin_as(&worker).await.is_err());
    client
        .admin_as(&admin)
        .await
        .expect("admin low-level access")
        .tooling()
        .list_categories(1)
        .await
        .expect("admin category read");

    let queued = client
        .add_buffered_as_in_group(
            &worker,
            &format!("secondary pending payload {suffix}"),
            &worker,
            Some("rbac-secondary-e2e"),
            None,
            Some(&group),
        )
        .await
        .expect("queue worker write");
    assert!(
        client
            .add_status_as(&viewer, &queued.pending_id)
            .await
            .is_err()
    );
    assert!(
        client
            .add_status_as(&worker, &queued.pending_id)
            .await
            .is_ok()
    );
    assert!(
        client
            .add_status_as(&admin, &queued.pending_id)
            .await
            .is_ok()
    );
    assert!(client.drain_notices_as(&viewer, &worker, 5).await.is_err());
    assert!(client.drain_notices_as(&worker, &worker, 5).await.is_ok());

    let (mut mcp, _) = McpClient::spawn();
    let session = format!("rbac-secondary-{suffix}");
    mcp.call_tool(
        "think_start",
        json!({
            "session_id": session,
            "initial_thought": "private worker reasoning",
            "actor_id": worker,
        }),
    );

    for (tool, arguments) in [
        (
            "think_status",
            json!({"session_id": session, "actor_id": viewer}),
        ),
        (
            "think_add",
            json!({
                "session_id": session,
                "content": "cross-principal tamper",
                "actor_id": viewer,
            }),
        ),
        (
            "think_conclude",
            json!({
                "session_id": session,
                "conclusion": "stolen conclusion",
                "actor_id": viewer,
            }),
        ),
        (
            "think_discard",
            json!({"session_id": session, "actor_id": viewer}),
        ),
    ] {
        let error = mcp.call_tool_expect_error(tool, arguments);
        assert!(
            error.contains("another actor") || error.contains("actor"),
            "{tool} must fail on actor mismatch: {error}"
        );
    }

    mcp.call_tool(
        "think_add",
        json!({
            "session_id": session,
            "content": "owner-controlled thought",
            "actor_id": worker,
        }),
    );
    mcp.call_tool(
        "think_conclude",
        json!({
            "session_id": session,
            "conclusion": "owner-controlled conclusion",
            "actor_id": worker,
        }),
    );
    let commit_error = mcp.call_tool_expect_error(
        "think_commit",
        json!({
            "session_id": session,
            "user_id": viewer,
            "actor_id": viewer,
            "group_id": group,
        }),
    );
    assert!(commit_error.contains("actor"), "commit must be actor-bound");
    mcp.call_tool(
        "think_discard",
        json!({"session_id": session, "actor_id": worker}),
    );

    client
        .admin_as(&admin)
        .await
        .expect("admin cleanup")
        .db()
        .execute_query::<serde_json::Value, _>(
            "deletePendingInput",
            &json!({"pending_id": queued.pending_id}),
        )
        .await
        .expect("delete pending fixture");

    rbac.revoke_as(&worker, Role::Worker, Some(&group), &admin)
        .await
        .expect("revoke worker");
    rbac.revoke_as(&viewer, Role::Viewer, Some(&group), &admin)
        .await
        .expect("revoke viewer");
    rbac.deactivate_group_as(&group, &admin)
        .await
        .expect("deactivate group");
    assert!(rbac.snapshot().await.expect("final snapshot").enabled);
}
