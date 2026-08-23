//! Active charter v1.0 C2/C4 persistence contract.
//!
//! This test is destructive only inside the disposable live E2E database: it
//! creates two isolated fixture memories and proves the public update tool
//! rejects both before changing content.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

mod common;
use common::{McpClient, db_query};

fn token() -> String {
    format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

fn add_fixture(memory_id: &str, owner: &str, source: &str, immutable: bool) {
    let now = chrono::Utc::now().to_rfc3339();
    let response = db_query(
        "addMemoryKeyedScopedProtected",
        &json!({
            "memory_id": memory_id,
            "content_key": format!("charter-e2e:{memory_id}"),
            "rbac_scope": "group:default",
            "user_id": owner,
            "content": format!("charter protected fixture {memory_id}"),
            "memory_type": "fact",
            "certainty": 100,
            "importance": 1,
            "created_at": now,
            "updated_at": now,
            "valid_from": now,
            "context_tags": "charter-protected-e2e",
            "source": source,
            "metadata": "{}",
            "immutable": i64::from(immutable)
        }),
    );
    assert!(
        response.get("error").is_none(),
        "fixture insert: {response}"
    );
}

fn contains_string(value: &serde_json::Value, needle: &str) -> bool {
    match value {
        serde_json::Value::String(value) => value == needle,
        serde_json::Value::Array(values) => {
            values.iter().any(|value| contains_string(value, needle))
        }
        serde_json::Value::Object(values) => {
            values.values().any(|value| contains_string(value, needle))
        }
        _ => false,
    }
}

#[test]
#[ignore = "needs HELIX_E2E=1 + disposable current-schema HelixDB"]
fn immutable_and_raw_input_reject_public_updates() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");
    let actor = std::env::var("HELIXIR_RBAC_ACTOR")
        .expect("HELIXIR_RBAC_ACTOR must name the disposable global admin");
    let run = token();
    let immutable_id = format!("charter_immutable_{run}");
    let raw_id = format!("charter_raw_{run}");
    let replacement_id = format!("charter_replacement_{run}");

    add_fixture(&immutable_id, &actor, "system_seed", true);
    add_fixture(&raw_id, &actor, "raw_input", false);
    add_fixture(&replacement_id, &actor, "charter_e2e", false);

    let (mut mcp, _) = McpClient::spawn();
    let rules = mcp.read_resource("memory://rules");
    assert!(
        rules.contains("ACTIVE v1.0"),
        "stale charter resource: {rules}"
    );

    for (memory_id, expected) in [
        (&immutable_id, "immutable_target"),
        (&raw_id, "raw_input_target"),
    ] {
        let error = mcp.call_tool_expect_error(
            "update_memory",
            json!({
                "actor_id": actor,
                "memory_id": memory_id,
                "new_content": "this mutation must never land",
                "user_id": actor
            }),
        );
        assert!(error.contains(expected), "wrong protection error: {error}");
        let persisted = db_query("getMemory", &json!({"memory_id": memory_id}));
        assert!(
            persisted["memory"]["content"]
                .as_str()
                .unwrap_or_default()
                .contains("charter protected fixture"),
            "protected content changed: {persisted}"
        );

        // Exercise the exact HQL backstops used by the add pipeline and
        // contradiction resolver. Even if a caller skips the Rust preflight,
        // a protected target cannot be updated or acquire SUPERSEDES.
        let _guarded_update = db_query(
            "updateMutableMemory",
            &json!({
                "memory_id": memory_id,
                "content": "atomic charter guard must reject this",
                "certainty": 100,
                "importance": 1,
                "updated_at": chrono::Utc::now().to_rfc3339()
            }),
        );
        let persisted = db_query("getMemory", &json!({"memory_id": memory_id}));
        assert!(
            persisted["memory"]["content"]
                .as_str()
                .unwrap_or_default()
                .contains("charter protected fixture"),
            "atomic update guard failed: {persisted}"
        );

        let _guarded_supersession = db_query(
            "addMutableMemorySupersession",
            &json!({
                "new_id": replacement_id,
                "old_id": memory_id,
                "reason": "atomic charter guard e2e",
                "superseded_at": chrono::Utc::now().to_rfc3339(),
                "is_contradiction": 0
            }),
        );
        let superseded = db_query(
            "getSupersededMemories",
            &json!({"memory_id": replacement_id}),
        );
        assert!(
            !contains_string(&superseded, memory_id),
            "atomic supersession guard failed: {superseded}"
        );
    }
}
