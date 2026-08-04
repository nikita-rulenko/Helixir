//! Live compatibility-profile bootstrap and implicit-routing contract.

use helixir::core::{HelixirClient, ONBOARDING_GROUP_ID, Role};
use helixir::llm::extractor::ExtractedMemory;

#[tokio::test]
#[ignore = "needs HELIX_E2E=1, an RBAC admin, deployed schema, and embeddings"]
async fn compatibility_bootstrap_is_idempotent_and_routes_omitted_groups() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");
    let admin = std::env::var("HELIXIR_RBAC_ACTOR").unwrap_or_else(|_| "codex".to_string());
    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");
    let rbac = client.rbac();
    let initial = rbac.snapshot().await.expect("initial policy");
    assert!(initial.enabled, "live E2E keeps RBAC enabled");
    assert!(initial.is_admin(&admin), "actor must be global admin");

    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let agent = format!("compat-agent-{suffix}");
    let registry_user = format!("registry-user-{suffix}");
    let project_group = format!("registry-project-{suffix}");
    let principals = vec![agent.clone()];
    let first = rbac
        .bootstrap_compatibility(&admin, &principals)
        .await
        .expect("first bootstrap");
    let second = rbac
        .bootstrap_compatibility(&admin, &principals)
        .await
        .expect("idempotent bootstrap");
    assert!(first.enabled_after && second.enabled_after);
    assert_eq!(first.memories_seen, second.memories_seen);
    assert!(rbac.compatibility_coverage_complete().await.unwrap());

    rbac.create_group_as(&project_group, "Registry E2E", "temporary", &admin)
        .await
        .expect("create project group");
    assert!(
        rbac.add_user_to_group(&registry_user, &project_group, Role::Viewer, &admin)
            .await
            .is_err(),
        "non-onboarded users cannot enter another group first"
    );

    rbac.add_user_to_group(&registry_user, ONBOARDING_GROUP_ID, Role::Worker, &admin)
        .await
        .expect("enroll registry user");
    let enrolled = rbac
        .principal_registry(&admin)
        .await
        .expect("principal registry")
        .into_iter()
        .find(|record| record.user_id == registry_user)
        .expect("registered user");
    assert!(enrolled.enrolled);
    assert!(enrolled.active_roles.iter().any(|role| {
        role.group_id.as_deref() == Some(ONBOARDING_GROUP_ID) && role.role == "worker"
    }));
    rbac.add_user_to_group(&registry_user, &project_group, Role::Viewer, &admin)
        .await
        .expect("assign enrolled user to project group");
    rbac.remove_user_from_group(&registry_user, &project_group, &admin)
        .await
        .expect("remove project membership");
    let revoked = rbac
        .remove_user_from_group(&registry_user, ONBOARDING_GROUP_ID, &admin)
        .await
        .expect("remove registry user");
    assert_eq!(revoked, ["worker"]);
    let removed = rbac
        .principal_registry(&admin)
        .await
        .expect("registry after removal")
        .into_iter()
        .find(|record| record.user_id == registry_user)
        .expect("removed user retained");
    assert!(!removed.enrolled);
    assert!(removed.role_history.iter().any(|role| {
        role.group_id.as_deref() == Some(ONBOARDING_GROUP_ID)
            && role.role == "worker"
            && !role.active
    }));
    assert!(
        rbac.compatibility_user_coverage_complete().await.unwrap(),
        "intentional offboarding keeps historical migration coverage"
    );
    rbac.bootstrap_compatibility(&admin, &principals)
        .await
        .expect("bootstrap after offboarding");
    let still_removed = rbac
        .principal_registry(&admin)
        .await
        .expect("registry after bootstrap replay")
        .into_iter()
        .find(|record| record.user_id == registry_user)
        .expect("offboarded user retained after replay");
    assert!(
        !still_removed.enrolled,
        "idempotent bootstrap must not reactivate an intentionally offboarded user"
    );

    let result = client
        .add_prepared_as_in_group(
            &agent,
            vec![ExtractedMemory {
                text: format!("compatibility implicitly routed fact {suffix}"),
                memory_type: "fact".to_string(),
                certainty: 95,
                importance: 60,
                entities: vec![],
                context: None,
            }],
            &agent,
            Some("rbac-compat-e2e"),
            None,
            None,
        )
        .await
        .expect("implicit compatibility write");
    assert!(
        client
            .add_prepared_as_in_group(
                &format!("unknown-{suffix}"),
                vec![ExtractedMemory {
                    text: format!("unauthorized self enrollment {suffix}"),
                    memory_type: "fact".to_string(),
                    certainty: 95,
                    importance: 60,
                    entities: vec![],
                    context: None,
                }],
                &format!("unknown-{suffix}"),
                Some("rbac-compat-e2e"),
                None,
                None,
            )
            .await
            .is_err(),
        "unknown principals must not self-enroll through a write"
    );
    let memory_id = result
        .memory_ids
        .first()
        .or_else(|| result.updated.first())
        .or_else(|| result.deduped.first())
        .expect("saved id")
        .clone();
    let groups = rbac
        .memory_group_map(std::slice::from_ref(&memory_id))
        .await
        .expect("memory groups");
    assert!(
        groups
            .get(&memory_id)
            .is_some_and(|ids| ids.contains(ONBOARDING_GROUP_ID))
    );
    assert!(
        rbac.visible_memory_ids(&agent, std::slice::from_ref(&memory_id))
            .await
            .expect("visibility")
            .expect("restricted principal")
            .contains(&memory_id)
    );

    rbac.bootstrap_compatibility(&admin, &principals)
        .await
        .expect("enabled bootstrap replay after admitted write");

    rbac.revoke_as(&agent, Role::Worker, Some(ONBOARDING_GROUP_ID), &admin)
        .await
        .expect("revoke fixture principal");
    rbac.deactivate_group_as(&project_group, &admin)
        .await
        .expect("deactivate project fixture");
    assert!(rbac.snapshot().await.expect("final policy").enabled);
}
