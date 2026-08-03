//! Enabled-state HelixDB RBAC and dedup-federation contract test.
//!
//! Run explicitly with `HELIX_E2E=1 HELIXIR_RBAC_ACTOR=<admin> cargo test
//! --test rbac_e2e -- --ignored --nocapture`. The test never disables RBAC.

use helixir::core::helixir_client::AddMemoryResult;
use helixir::core::{HelixirClient, Role};
use helixir::llm::extractor::ExtractedMemory;

fn token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn fact(text: impl Into<String>) -> ExtractedMemory {
    ExtractedMemory {
        text: text.into(),
        memory_type: "fact".to_string(),
        certainty: 95,
        importance: 60,
        entities: vec![],
        context: None,
    }
}

fn saved_id(result: &AddMemoryResult) -> Option<String> {
    result
        .memory_ids
        .first()
        .or_else(|| result.deduped.first())
        .cloned()
}

async fn content_key(client: &HelixirClient, memory_id: &str) -> String {
    let response: serde_json::Value = client
        .db()
        .execute_query("getMemory", &serde_json::json!({"memory_id": memory_id}))
        .await
        .expect("get memory");
    response
        .get("memory")
        .and_then(|memory| memory.get("content_key"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
#[ignore = "needs HELIX_E2E=1, enabled RBAC, models, and deployed schema"]
async fn rbac_dedup_federation_preserves_history_and_isolates_future_writes() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let admin = std::env::var("HELIXIR_RBAC_ACTOR").unwrap_or_else(|_| "claude".to_string());
    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");
    let rbac = client.rbac();
    let initial = rbac.snapshot().await.expect("initial RBAC snapshot");
    assert!(initial.enabled, "E2E requires RBAC to remain enabled");
    assert!(initial.is_admin(&admin), "HELIXIR_RBAC_ACTOR must be admin");

    let suffix = token();
    println!("RBAC E2E fixture suffix: {suffix}");
    let group_a = format!("rbac_e2e_a_{suffix}");
    let group_b = format!("rbac_e2e_b_{suffix}");
    let group_c = format!("rbac_e2e_c_{suffix}");
    let dedup_group = format!("rbac_e2e_dev_{suffix}");
    let worker_a = format!("worker_a_{suffix}");
    let worker_b = format!("worker_b_{suffix}");
    let worker_c = format!("worker_c_{suffix}");
    let multi_group_worker = format!("multi_group_worker_{suffix}");
    let viewer_a = format!("viewer_a_{suffix}");
    let viewer_b = format!("viewer_b_{suffix}");
    let viewer_c = format!("viewer_c_{suffix}");

    for group in [&group_a, &group_b, &group_c] {
        rbac.create_group_as(group, group, "RBAC dedup E2E", &admin)
            .await
            .expect("create group");
    }
    rbac.create_dedup_group_as(&dedup_group, &dedup_group, "E2E federation", &admin)
        .await
        .expect("create dedup group");
    rbac.attach_group_to_dedup_as(&group_a, &dedup_group, &admin)
        .await
        .expect("attach A");
    rbac.attach_group_to_dedup_as(&group_b, &dedup_group, &admin)
        .await
        .expect("attach B");

    for (user, role, group) in [
        (&worker_a, Role::Worker, &group_a),
        (&worker_b, Role::Worker, &group_b),
        (&worker_c, Role::Worker, &group_c),
        (&multi_group_worker, Role::Worker, &group_a),
        (&multi_group_worker, Role::Worker, &group_c),
        (&viewer_a, Role::Viewer, &group_a),
        (&viewer_b, Role::Viewer, &group_b),
        (&viewer_c, Role::Viewer, &group_c),
    ] {
        rbac.grant(user, role, Some(group), &admin)
            .await
            .expect("grant role");
    }

    let shared_text = format!("federated exact fact {suffix}");
    let a = client
        .add_prepared_as_in_group(
            &worker_a,
            vec![fact(&shared_text)],
            &worker_a,
            Some("rbac-e2e"),
            None,
            Some(&group_a),
        )
        .await
        .expect("A write");
    let b = client
        .add_prepared_as_in_group(
            &worker_b,
            vec![fact(&shared_text)],
            &worker_b,
            Some("rbac-e2e"),
            None,
            Some(&group_b),
        )
        .await
        .expect("B write");
    let c = client
        .add_prepared_as_in_group(
            &worker_c,
            vec![fact(&shared_text)],
            &worker_c,
            Some("rbac-e2e"),
            None,
            Some(&group_c),
        )
        .await
        .expect("C write");
    let a_id = saved_id(&a).expect("A saved memory id");
    let b_id = saved_id(&b).expect("B saved memory id");
    let c_id = saved_id(&c).expect("C saved memory id");
    assert_eq!(
        content_key(&client, &a_id).await,
        content_key(&client, &b_id).await
    );
    assert_ne!(
        content_key(&client, &a_id).await,
        content_key(&client, &c_id).await
    );

    let all_ids = vec![a_id.clone(), b_id.clone(), c_id.clone()];
    let a_visible = rbac
        .visible_memory_ids(&viewer_a, &all_ids)
        .await
        .expect("A visibility")
        .expect("restricted viewer");
    assert!(a_visible.contains(&a_id) && a_visible.contains(&b_id));
    assert!(!a_visible.contains(&c_id));
    let c_visible = rbac
        .visible_memory_ids(&viewer_c, &all_ids)
        .await
        .expect("C visibility")
        .expect("restricted viewer");
    assert_eq!(c_visible, std::collections::HashSet::from([c_id.clone()]));

    assert!(
        client
            .add_prepared_as_in_group(
                &multi_group_worker,
                vec![fact(format!("ambiguous multi-group fact {suffix}"))],
                &multi_group_worker,
                Some("rbac-e2e"),
                None,
                None,
            )
            .await
            .is_err(),
        "a multi-group worker must select one concrete access group"
    );
    let multi_group_result = client
        .add_prepared_as_in_group(
            &multi_group_worker,
            vec![fact(format!("isolated multi-group fact {suffix}"))],
            &multi_group_worker,
            Some("rbac-e2e"),
            None,
            Some(&group_c),
        )
        .await
        .expect("explicit multi-group write");
    let multi_group_id = saved_id(&multi_group_result).expect("multi-group saved memory id");
    assert!(
        rbac.visible_memory_ids(&viewer_c, std::slice::from_ref(&multi_group_id))
            .await
            .expect("multi-group C visibility")
            .expect("restricted viewer")
            .contains(&multi_group_id)
    );
    assert!(
        !rbac
            .visible_memory_ids(&viewer_a, std::slice::from_ref(&multi_group_id))
            .await
            .expect("multi-group A visibility")
            .expect("restricted viewer")
            .contains(&multi_group_id),
        "explicit group C write must not leak to the owner's group A"
    );

    assert!(
        client
            .add_prepared_as_in_group(
                &viewer_a,
                vec![fact(format!("viewer denied {suffix}"))],
                &viewer_a,
                Some("rbac-e2e"),
                None,
                Some(&group_a),
            )
            .await
            .is_err(),
        "viewer must remain read-only"
    );

    rbac.detach_group_from_dedup_as(&group_b, &admin)
        .await
        .expect("detach B");
    assert!(
        rbac.visible_memory_ids(&viewer_b, std::slice::from_ref(&a_id))
            .await
            .expect("historical visibility")
            .expect("restricted viewer")
            .contains(&a_id),
        "B retains historical access"
    );
    assert!(
        client
            .update_as(
                &worker_a,
                &a_id,
                &format!("mutated historical fact {suffix}")
            )
            .await
            .is_err(),
        "historical federation memory must not mutate in place"
    );

    let future_text = format!("post-detach exact fact {suffix}");
    let future_a_result = client
        .add_prepared_as_in_group(
            &worker_a,
            vec![fact(&future_text)],
            &worker_a,
            Some("rbac-e2e"),
            None,
            Some(&group_a),
        )
        .await
        .expect("future A write");
    let future_a = saved_id(&future_a_result).expect("future A saved memory id");
    let future_b_result = client
        .add_prepared_as_in_group(
            &worker_b,
            vec![fact(&future_text)],
            &worker_b,
            Some("rbac-e2e"),
            None,
            Some(&group_b),
        )
        .await
        .expect("future B write");
    let future_b = saved_id(&future_b_result).expect("future B saved memory id");
    assert_ne!(
        content_key(&client, &future_a).await,
        content_key(&client, &future_b).await
    );
    assert!(
        !rbac
            .visible_memory_ids(&viewer_b, std::slice::from_ref(&future_a))
            .await
            .expect("future visibility")
            .expect("restricted viewer")
            .contains(&future_a),
        "detached B must not see future federation memories"
    );

    rbac.attach_group_to_dedup_as(&group_c, &dedup_group, &admin)
        .await
        .expect("attach C with history backfill");
    assert!(
        rbac.visible_memory_ids(&viewer_c, std::slice::from_ref(&future_a))
            .await
            .expect("joined history visibility")
            .expect("restricted viewer")
            .contains(&future_a),
        "joining a federation grants its existing history"
    );

    for (user, role, group) in [
        (&worker_a, Role::Worker, &group_a),
        (&worker_b, Role::Worker, &group_b),
        (&worker_c, Role::Worker, &group_c),
        (&multi_group_worker, Role::Worker, &group_a),
        (&multi_group_worker, Role::Worker, &group_c),
        (&viewer_a, Role::Viewer, &group_a),
        (&viewer_b, Role::Viewer, &group_b),
        (&viewer_c, Role::Viewer, &group_c),
    ] {
        rbac.revoke_as(user, role, Some(group), &admin)
            .await
            .expect("revoke role");
    }
    for group in [&group_a, &group_c] {
        rbac.detach_group_from_dedup_as(group, &admin)
            .await
            .expect("detach cleanup");
    }
    for group in [&group_a, &group_b, &group_c] {
        rbac.deactivate_group_as(group, &admin)
            .await
            .expect("deactivate group");
    }
    rbac.deactivate_dedup_group_as(&dedup_group, &admin)
        .await
        .expect("deactivate dedup group");
    assert!(rbac.snapshot().await.expect("final snapshot").enabled);
}
