//! Fresh-store and trusted-mode upgrade coverage for the onboarding profile.
//!
//! Each scenario requires its own empty HelixDB volume. Run with
//! `HELIX_E2E_FRESH=1 HELIX_E2E_SCENARIO=fresh|legacy-upgrade` and point
//! `HELIX_HOST` / `HELIX_PORT` at that disposable instance.

use helixir::core::{HelixirClient, ONBOARDING_GROUP_ID, Role};
use helixir::llm::extractor::ExtractedMemory;

fn saved_id(result: &helixir::core::helixir_client::AddMemoryResult) -> Option<String> {
    result
        .memory_ids
        .first()
        .or_else(|| result.updated.first())
        .or_else(|| result.deduped.first())
        .cloned()
}

async fn memory(client: &HelixirClient, actor: &str, memory_id: &str) -> serde_json::Value {
    client
        .admin_as(actor)
        .await
        .expect("RBAC admin")
        .db()
        .execute_query("getMemory", &serde_json::json!({"memory_id": memory_id}))
        .await
        .expect("get memory")
}

#[tokio::test]
#[ignore = "needs a disposable empty HelixDB instance and HELIX_E2E_FRESH=1"]
async fn fresh_store_and_legacy_upgrade_bootstrap() {
    assert_eq!(std::env::var("HELIX_E2E_FRESH").unwrap_or_default(), "1");
    let scenario = std::env::var("HELIX_E2E_SCENARIO").expect("HELIX_E2E_SCENARIO");
    assert!(matches!(scenario.as_str(), "fresh" | "legacy-upgrade"));

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");
    let rbac = client.rbac();
    let initial = rbac.snapshot().await.expect("initial policy");
    assert!(!initial.enabled, "fixture must start in trusted mode");

    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let operator = format!("bootstrap-admin-{suffix}");
    let agent = format!("bootstrap-agent-{suffix}");
    let legacy_owner = format!("legacy-owner-{suffix}");
    let legacy_text = format!("legacy compatibility fact {suffix}");

    let legacy_id = if scenario == "legacy-upgrade" {
        let result = client
            .add_prepared_as_in_group(
                &legacy_owner,
                vec![ExtractedMemory {
                    text: legacy_text.clone(),
                    memory_type: "fact".to_string(),
                    certainty: 95,
                    importance: 60,
                    entities: vec![],
                    context: None,
                }],
                &legacy_owner,
                Some("rbac-bootstrap-e2e"),
                None,
                None,
            )
            .await
            .expect("trusted-mode seed");
        Some(saved_id(&result).expect("legacy memory id"))
    } else {
        None
    };

    let report = rbac
        .bootstrap_compatibility(&operator, std::slice::from_ref(&agent))
        .await
        .expect("bootstrap compatibility profile");
    assert!(!report.enabled_before && report.enabled_after);
    assert_eq!(report.group_id, ONBOARDING_GROUP_ID);
    assert!(report.principals_enrolled.contains(&operator));
    assert!(report.principals_enrolled.contains(&agent));

    let policy = rbac.snapshot().await.expect("enabled policy");
    assert!(policy.enabled && policy.is_admin(&operator));
    assert!(
        policy
            .users
            .get(&agent)
            .and_then(|binding| binding.groups.get(ONBOARDING_GROUP_ID))
            .is_some_and(|roles| roles.contains(&Role::GroupAdmin))
    );
    assert!(rbac.compatibility_coverage_complete().await.unwrap());
    assert!(rbac.compatibility_user_coverage_complete().await.unwrap());

    if let Some(legacy_id) = legacy_id {
        let groups = rbac
            .memory_group_map(std::slice::from_ref(&legacy_id))
            .await
            .expect("legacy memory groups");
        assert!(
            groups
                .get(&legacy_id)
                .is_some_and(|ids| ids.contains(ONBOARDING_GROUP_ID))
        );
        assert!(
            rbac.visible_memory_ids(&agent, std::slice::from_ref(&legacy_id))
                .await
                .expect("legacy visibility")
                .expect("restricted agent")
                .contains(&legacy_id)
        );

        let replay = client
            .add_prepared_as_in_group(
                &agent,
                vec![ExtractedMemory {
                    text: legacy_text,
                    memory_type: "fact".to_string(),
                    certainty: 95,
                    importance: 60,
                    entities: vec![],
                    context: None,
                }],
                &agent,
                Some("rbac-bootstrap-e2e"),
                None,
                None,
            )
            .await
            .expect("enabled compatibility replay");
        let replay_id = saved_id(&replay).expect("replay affected memory id");
        let legacy = memory(&client, &operator, &legacy_id).await;
        let replay = memory(&client, &operator, &replay_id).await;
        assert_eq!(
            legacy["memory"]["content_key"],
            replay["memory"]["content_key"]
        );
        assert_eq!(legacy["memory"]["user_id"], legacy_owner);
    }
}
