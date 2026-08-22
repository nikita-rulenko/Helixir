//! Disposable HelixDB contract for remote-client enrollment and memory scopes.
//!
//! This suite deliberately avoids LLM/NLI/embedding calls. It owns the graph
//! boundary required by the pre-release client gate: concurrent admission,
//! one principal in multiple groups, and fail-closed memory visibility.

use std::{collections::HashSet, sync::Arc};

use helixir::{
    core::{RbacManager, Role},
    db::HelixClient as DbClient,
};
use serde_json::{Value, json};

struct ScopedMemory<'a> {
    memory_id: &'a str,
    owner_id: &'a str,
    content: &'a str,
    content_key: &'a str,
    rbac_scope: &'a str,
    group_id: &'a str,
}

fn nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

async fn seed_scoped_memory(
    db: &DbClient,
    rbac: &RbacManager,
    memory: ScopedMemory<'_>,
    actor_id: &str,
) {
    let now = chrono::Utc::now().to_rfc3339();
    db.execute_query::<Value, _>(
        "addMemoryKeyedScoped",
        &json!({
            "memory_id": memory.memory_id,
            "content_key": memory.content_key,
            "rbac_scope": memory.rbac_scope,
            "user_id": memory.owner_id,
            "content": memory.content,
            "memory_type": "fact",
            "certainty": 95,
            "importance": 60,
            "created_at": now,
            "updated_at": now,
            "valid_from": now,
            "context_tags": "pre-release-client-gate",
            "source": "pre-release-fixture",
            "metadata": "{}",
        }),
    )
    .await
    .expect("seed scoped memory");
    rbac.link_memory_to_group(memory.memory_id, Some(memory.group_id), actor_id)
        .await
        .expect("materialize group visibility");
}

fn count_rows(response: &Value, collection: &str, predicate: impl Fn(&Value) -> bool) -> usize {
    response
        .get(collection)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|row| predicate(row))
        .count()
}

#[tokio::test]
#[ignore = "requires HELIX_E2E=1 and a fresh disposable HelixDB v2.3.5 instance"]
async fn concurrent_clients_preserve_group_scoped_visibility() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let host = std::env::var("HELIX_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("HELIX_PORT")
        .unwrap_or_else(|_| "6969".to_string())
        .parse()
        .expect("HELIX_PORT");
    let admin =
        std::env::var("HELIXIR_RBAC_ACTOR").unwrap_or_else(|_| "pre-release-admin".to_string());
    let db = Arc::new(DbClient::new(&host, port).expect("HelixDB client"));
    db.connect().await.expect("connect HelixDB");
    let rbac = RbacManager::new(Arc::clone(&db));
    rbac.bootstrap_compatibility(&admin, &[])
        .await
        .expect("bootstrap permanent RBAC");

    let suffix = nonce();
    let client_a = format!("client-a-{suffix}");
    let client_b = format!("client-b-{suffix}");
    let shared_principal = format!("shared-{suffix}");
    let group_a = format!("scope-a-{suffix}");
    let group_b = format!("scope-b-{suffix}");

    let (enroll_a, enroll_b) = tokio::join!(
        rbac.self_enroll_client(&client_a),
        rbac.self_enroll_client(&client_b)
    );
    assert!(enroll_a.expect("enroll client A").created);
    assert!(enroll_b.expect("enroll client B").created);

    let (race_one, race_two) = tokio::join!(
        rbac.self_enroll_client(&shared_principal),
        rbac.self_enroll_client(&shared_principal)
    );
    race_one.expect("first same-principal enrollment");
    race_two.expect("second same-principal enrollment");

    let users: Value = db
        .execute_query("getAllUsers", &json!({}))
        .await
        .expect("list users");
    assert_eq!(
        count_rows(&users, "users", |row| row["user_id"] == shared_principal),
        1,
        "concurrent self-enrollment must create one User node"
    );
    let assignments: Value = db
        .execute_query("getAllRbacAssignments", &json!({}))
        .await
        .expect("list assignments");
    assert_eq!(
        count_rows(&assignments, "assignments", |row| {
            row["subject_id"] == shared_principal
                && row["group_id"] == "onboarding"
                && row["role"] == "worker"
                && row["active"] == 1
        }),
        1,
        "concurrent self-enrollment must create one active onboarding grant"
    );

    if let Ok(transport_principal) = std::env::var("HELIXIR_CLIENT_GATE_SHARED_PRINCIPAL") {
        assert_eq!(
            count_rows(&users, "users", |row| {
                row["user_id"] == transport_principal.as_str()
            }),
            1,
            "the same principal enrolled concurrently through two real MCP clients must create one User node"
        );
        assert_eq!(
            count_rows(&assignments, "assignments", |row| {
                row["subject_id"] == transport_principal.as_str()
                    && row["group_id"] == "onboarding"
                    && row["role"] == "worker"
                    && row["active"] == 1
            }),
            1,
            "the same principal enrolled concurrently through two real MCP clients must create one active onboarding grant"
        );
        let agents: Value = db
            .execute_query("listAgents", &json!({}))
            .await
            .expect("list agent presence rows");
        assert_eq!(
            count_rows(&agents, "agents", |row| {
                row["agent_id"] == transport_principal.as_str()
                    && row["principal_id"] == transport_principal.as_str()
            }),
            0,
            "RBAC enrollment is admission, not presence; only explicit heartbeat or an attributed write may create Agent rows"
        );
    }

    for group in [&group_a, &group_b] {
        rbac.create_group_as(group, group, "pre-release visibility fixture", &admin)
            .await
            .expect("create group");
    }
    for (principal, group) in [
        (&client_a, &group_a),
        (&client_b, &group_b),
        (&shared_principal, &group_a),
        (&shared_principal, &group_b),
    ] {
        rbac.grant(principal, Role::Worker, Some(group), &admin)
            .await
            .expect("grant group worker");
    }

    assert!(
        rbac.authorize_and_resolve_write_scope(&client_a, &client_a, Some(&group_b))
            .await
            .is_err(),
        "client A must not write into client B's group"
    );
    let shared_scope_a = rbac
        .authorize_and_resolve_write_scope(&shared_principal, &shared_principal, Some(&group_a))
        .await
        .expect("shared principal scope A");
    let shared_scope_b = rbac
        .authorize_and_resolve_write_scope(&shared_principal, &shared_principal, Some(&group_b))
        .await
        .expect("shared principal scope B");
    assert_ne!(
        shared_scope_a.scope.fingerprint_scope(),
        shared_scope_b.scope.fingerprint_scope(),
        "one owner in two isolated groups must receive two dedup namespaces"
    );

    let memory_a = format!("mem_a_{}", &suffix[..12]);
    let memory_b = format!("mem_b_{}", &suffix[..12]);
    let shared_a = format!("mem_shared_a_{}", &suffix[..12]);
    let shared_b = format!("mem_shared_b_{}", &suffix[..12]);
    let identical_content = format!("same owner scoped fact {suffix}");
    seed_scoped_memory(
        &db,
        &rbac,
        ScopedMemory {
            memory_id: &memory_a,
            owner_id: &client_a,
            content: &format!("client A private fact {suffix}"),
            content_key: &format!("key-a-{suffix}"),
            rbac_scope: &format!("rbac:group:{group_a}"),
            group_id: &group_a,
        },
        &admin,
    )
    .await;
    seed_scoped_memory(
        &db,
        &rbac,
        ScopedMemory {
            memory_id: &memory_b,
            owner_id: &client_b,
            content: &format!("client B private fact {suffix}"),
            content_key: &format!("key-b-{suffix}"),
            rbac_scope: &format!("rbac:group:{group_b}"),
            group_id: &group_b,
        },
        &admin,
    )
    .await;
    seed_scoped_memory(
        &db,
        &rbac,
        ScopedMemory {
            memory_id: &shared_a,
            owner_id: &shared_principal,
            content: &identical_content,
            content_key: &format!("shared-a-{suffix}"),
            rbac_scope: &format!("rbac:group:{group_a}"),
            group_id: &group_a,
        },
        &admin,
    )
    .await;
    seed_scoped_memory(
        &db,
        &rbac,
        ScopedMemory {
            memory_id: &shared_b,
            owner_id: &shared_principal,
            content: &identical_content,
            content_key: &format!("shared-b-{suffix}"),
            rbac_scope: &format!("rbac:group:{group_b}"),
            group_id: &group_b,
        },
        &admin,
    )
    .await;

    let all_ids = vec![
        memory_a.clone(),
        memory_b.clone(),
        shared_a.clone(),
        shared_b.clone(),
    ];
    let visible_a = rbac
        .visible_memory_ids(&client_a, &all_ids)
        .await
        .expect("client A visibility")
        .expect("client A is restricted");
    let visible_b = rbac
        .visible_memory_ids(&client_b, &all_ids)
        .await
        .expect("client B visibility")
        .expect("client B is restricted");
    assert_eq!(visible_a, HashSet::from([memory_a, shared_a]));
    assert_eq!(visible_b, HashSet::from([memory_b, shared_b]));

    let visible_shared = rbac
        .visible_memory_ids(&shared_principal, &all_ids)
        .await
        .expect("shared-principal visibility")
        .expect("shared principal is not global admin");
    assert_eq!(visible_shared, HashSet::from_iter(all_ids));
}
