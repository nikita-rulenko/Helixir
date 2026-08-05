//! Fresh-store, pre-RBAC upgrade, and interrupted-resume bootstrap coverage.
//!
//! Each scenario requires its own empty HelixDB volume. Run with
//! `HELIX_E2E_FRESH=1 HELIX_E2E_SCENARIO=fresh|legacy-upgrade|interrupted-legacy` and point
//! `HELIX_HOST` / `HELIX_PORT` at that disposable instance. The
//! `interrupted-legacy` case seeds a persisted `migrating` checkpoint with
//! enforcement already enabled, then proves that bootstrap converges forward.

use helixir::core::{
    DEFAULT_GROUP_ID, HelixirClient, ONBOARDING_GROUP_ID, RbacMigrationKind, RbacMigrationState,
    Role,
};
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
    assert!(matches!(
        scenario.as_str(),
        "fresh" | "legacy-upgrade" | "interrupted-legacy"
    ));

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

    let legacy_upgrade = scenario != "fresh";
    let legacy_id = if legacy_upgrade {
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

    if scenario == "interrupted-legacy" {
        rbac.create_group_as(
            DEFAULT_GROUP_ID,
            "Partial default",
            "failure fixture",
            &operator,
        )
        .await
        .expect("create partial default group");
        rbac.grant(&operator, Role::Admin, None, &operator)
            .await
            .expect("grant partial operator");
        let admin = client
            .admin_as(&operator)
            .await
            .expect("partial admin surface");
        admin
            .db()
            .execute_query::<serde_json::Value, _>(
                "setRbacMigrationState",
                &serde_json::json!({
                    "migration_state": "migrating",
                    "migration_kind": "legacy",
                    "updated_at": chrono::Utc::now().to_rfc3339(),
                    "updated_by": operator,
                }),
            )
            .await
            .expect("persist interrupted checkpoint");
        admin
            .db()
            .execute_query::<serde_json::Value, _>(
                "setRbacEnabled",
                &serde_json::json!({
                    "enabled": 1,
                    "updated_at": chrono::Utc::now().to_rfc3339(),
                    "updated_by": operator,
                }),
            )
            .await
            .expect("simulate interruption after enforcement enabled");
    }

    let report = rbac
        .bootstrap_compatibility(&operator, std::slice::from_ref(&agent))
        .await
        .expect("bootstrap compatibility profile");
    assert_eq!(report.enabled_before, scenario == "interrupted-legacy");
    assert!(report.enabled_after);
    assert_eq!(report.group_id, DEFAULT_GROUP_ID);
    assert_eq!(report.onboarding_group_id, ONBOARDING_GROUP_ID);
    assert_eq!(
        report.migration_kind,
        if legacy_upgrade {
            RbacMigrationKind::Legacy
        } else {
            RbacMigrationKind::Fresh
        }
    );
    assert!(report.principals_enrolled.contains(&operator));
    assert!(report.principals_enrolled.contains(&agent));

    let policy = rbac.snapshot().await.expect("enabled policy");
    assert!(policy.enabled && policy.is_admin(&operator));
    assert_eq!(policy.migration_state, RbacMigrationState::Active);
    let (expected_group, expected_role) = if legacy_upgrade {
        (DEFAULT_GROUP_ID, Role::GroupAdmin)
    } else {
        (ONBOARDING_GROUP_ID, Role::Worker)
    };
    assert!(
        policy
            .users
            .get(&agent)
            .and_then(|binding| binding.groups.get(expected_group))
            .is_some_and(|roles| roles.contains(&expected_role))
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
                .is_some_and(|ids| ids.contains(DEFAULT_GROUP_ID))
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
