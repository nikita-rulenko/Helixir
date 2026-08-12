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
                vec![fact(legacy_text.clone())],
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

    let legacy_moirai = if let Some(effect_id) = legacy_id.as_ref() {
        let cause = client
            .add_prepared_as_in_group(
                &legacy_owner,
                vec![fact(format!("legacy Lachesis cause {suffix}"))],
                &legacy_owner,
                Some("rbac-bootstrap-e2e"),
                None,
                None,
            )
            .await
            .expect("legacy cause seed");
        let cause_id = saved_id(&cause).expect("legacy cause id");
        let insight = client
            .add_prepared_as_in_group(
                "helixir",
                vec![ExtractedMemory {
                    text: format!("legacy generated insight {suffix}"),
                    memory_type: "opinion".to_string(),
                    certainty: 70,
                    importance: 65,
                    entities: vec![],
                    context: None,
                }],
                "helixir",
                None,
                Some("moira-insight"),
                None,
            )
            .await
            .expect("legacy Moirai insight seed");
        let insight_id = saved_id(&insight).expect("legacy insight id");
        let db = client
            .admin_as(&operator)
            .await
            .expect("trusted-mode maintenance surface");
        db.db()
            .execute_query::<serde_json::Value, _>(
                "addMemoryRelation",
                &serde_json::json!({
                    "source_id": effect_id,
                    "target_id": insight_id,
                    "relation_type": "SUPPORTS",
                    "strength": 60i64,
                    "created_at": chrono::Utc::now().to_rfc3339(),
                    "metadata": "atropos-legacy",
                }),
            )
            .await
            .expect("legacy Atropos provenance");
        db.db()
            .execute_query::<serde_json::Value, _>(
                "addMemoryRelation",
                &serde_json::json!({
                    "source_id": effect_id,
                    "target_id": insight_id,
                    "relation_type": "RELATES_TO",
                    "strength": 55i64,
                    "created_at": chrono::Utc::now().to_rfc3339(),
                    "metadata": "ordinary-relation-fixture",
                }),
            )
            .await
            .expect("ordinary relation sharing the legacy target");
        db.db()
            .execute_query::<serde_json::Value, _>(
                "addMemoryCausation",
                &serde_json::json!({
                    "from_id": effect_id,
                    "to_id": cause_id,
                    "strength": 80i64,
                    "reasoning_id": "lachesis-stitch",
                }),
            )
            .await
            .expect("legacy Lachesis edge");
        Some((insight_id, effect_id.clone()))
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
    assert_eq!(report.moirai_group_id, helixir::core::MOIRAI_GROUP_ID);
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

    assert!(
        rbac.grant(&agent, Role::TeamLead, Some(DEFAULT_GROUP_ID), &operator)
            .await
            .is_err(),
        "new teamlead grants must be rejected"
    );
    let legacy_assignment_id = format!("legacy_teamlead_{suffix}");
    client
        .admin_as(&operator)
        .await
        .expect("admin surface")
        .db()
        .execute_query::<serde_json::Value, _>(
            "grantRbacRole",
            &serde_json::json!({
                "assignment_id": legacy_assignment_id,
                "subject_id": agent,
                "role": "teamlead",
                "group_id": DEFAULT_GROUP_ID,
                "granted_by": "legacy-fixture",
                "created_at": chrono::Utc::now().to_rfc3339(),
                "metadata": "legacy-fixture",
            }),
        )
        .await
        .expect("seed legacy teamlead assignment");
    let migration = rbac
        .migrate_teamleads(&operator)
        .await
        .expect("migrate legacy teamlead");
    assert_eq!(migration.migrated, 1);
    let migrated_policy = rbac.snapshot().await.expect("migrated policy");
    let migrated_roles = &migrated_policy.users[&agent].groups[DEFAULT_GROUP_ID];
    assert!(migrated_roles.contains(&Role::GroupAdmin));
    assert!(!migrated_roles.contains(&Role::TeamLead));

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

    if let Some((legacy_insight_id, effect_id)) = legacy_moirai {
        let groups = rbac
            .memory_group_map(std::slice::from_ref(&legacy_insight_id))
            .await
            .expect("Moirai group projection");
        assert_eq!(
            groups.get(&legacy_insight_id),
            Some(&std::collections::HashSet::from([
                helixir::core::MOIRAI_GROUP_ID.to_string()
            ]))
        );
        assert!(
            !rbac
                .visible_memory_ids(&agent, std::slice::from_ref(&legacy_insight_id))
                .await
                .expect("Moirai visibility")
                .expect("restricted compatibility agent")
                .contains(&legacy_insight_id),
            "default groupadmin must not read the Moirai layer"
        );

        let db = client.admin_as(&operator).await.expect("admin surface");
        let witnesses: serde_json::Value = db
            .db()
            .execute_query(
                "getMoiraiWitnesses",
                &serde_json::json!({"insight_id": legacy_insight_id}),
            )
            .await
            .expect("migrated Atropos provenance");
        assert!(
            witnesses["witnesses"]
                .as_array()
                .is_some_and(|rows| !rows.is_empty())
        );
        let relations: serde_json::Value = db
            .db()
            .execute_query(
                "getMemoryOutgoingRelations",
                &serde_json::json!({"memory_id": effect_id}),
            )
            .await
            .expect("post-migration relations");
        assert!(
            relations["because_out"]
                .as_array()
                .is_none_or(Vec::is_empty),
            "legacy generated BECAUSE must leave the ordinary graph"
        );
        assert!(
            relations["relations_out"].as_array().is_some_and(|edges| {
                edges
                    .iter()
                    .any(|edge| edge["relation_type"] == "RELATES_TO")
            }),
            "Moirai repair must preserve unrelated generic relations"
        );
        let stitches: serde_json::Value = db
            .db()
            .execute_query(
                "searchByContextTag",
                &serde_json::json!({"tag": "moira-stitch", "limit": 100i64}),
            )
            .await
            .expect("migrated Lachesis hypotheses");
        let stitch_ids = stitches["memories"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row["memory_id"].as_str().map(str::to_string))
            .collect::<Vec<_>>();
        assert!(!stitch_ids.is_empty(), "legacy stitch must be reified");
        let stitch_groups = rbac
            .memory_group_map(&stitch_ids)
            .await
            .expect("stitch group projection");
        assert!(stitch_ids.iter().all(|id| {
            stitch_groups.get(id).is_some_and(|groups| {
                groups
                    == &std::collections::HashSet::from(
                        [helixir::core::MOIRAI_GROUP_ID.to_string()],
                    )
            })
        }));
        for stitch_id in &stitch_ids {
            let stitch = memory(&client, &operator, stitch_id).await;
            let internal_id = stitch["memory"]["id"].as_str().expect("stitch internal id");
            let embedding: serde_json::Value = db
                .db()
                .execute_query(
                    "getMemoryEmbedding",
                    &serde_json::json!({"memory_id": internal_id}),
                )
                .await
                .expect("migrated stitch embedding");
            assert!(!embedding["embedding"].is_null());
        }
    }
}
