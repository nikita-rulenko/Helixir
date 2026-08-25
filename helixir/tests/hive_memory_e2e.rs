//! Deterministic Hive consensus end-to-end test against live HelixDB.
//!
//! This test owns the physical fingerprint-group invariant: two author-level
//! memories with the same scoped `content_key` must project two distinct
//! holders. The full LLM extraction and MCP write path is covered separately
//! by `mcp_multi_consumer_e2e::multi_consumer_collective_invariants`; keeping
//! extraction out of this test prevents model wording variance from changing
//! the input to the consensus assertion.
//!
//! **Not run by default** (`#[ignore]`). Requires a disposable HelixDB:
//! `HELIX_HOST`, `HELIX_PORT`, and `HELIX_E2E=1`.

use helixir::db::HelixClient;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Deserialize)]
struct CountResult {
    #[serde(default)]
    count: i64,
}

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 and a disposable HelixDB; see module doc"]
async fn hive_cross_user_collective_link_e2e() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );

    let token = format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    let content_key = format!("e2e:hive:rbac:group:default:{token}");
    let content = format!("Service atlas{token} ships every Thursday at 14:00 UTC.");
    let now = chrono::Utc::now().to_rfc3339();
    let users = [format!("e2e_hive_{token}_a"), format!("e2e_hive_{token}_b")];

    let host = std::env::var("HELIX_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("HELIX_PORT")
        .unwrap_or_else(|_| "6969".to_string())
        .parse::<u16>()
        .expect("HELIX_PORT must be a u16");
    let db = HelixClient::new(&host, port).expect("HelixClient::new");
    db.connect().await.expect("connect to disposable HelixDB");

    for (index, user_id) in users.iter().enumerate() {
        let memory_id = format!("mem_hive_{token}_{index}");
        db.execute_query::<Value, _>("ensureUser", &json!({"user_id": user_id, "name": user_id}))
            .await
            .expect("ensure Hive holder");
        db.execute_query::<Value, _>(
            "addMemoryKeyedScopedProtected",
            &json!({
                "memory_id": memory_id,
                "content_key": content_key,
                "rbac_scope": "rbac:group:default",
                "user_id": user_id,
                "content": content,
                "memory_type": "fact",
                "certainty": 100,
                "importance": 80,
                "created_at": now,
                "updated_at": now,
                "valid_from": now,
                "context_tags": "e2e,hive",
                "source": "hive_memory_e2e",
                "metadata": "{}",
                "immutable": 0
            }),
        )
        .await
        .expect("create exact Hive memory");
        db.execute_query::<Value, _>(
            "linkUserToMemoryWithStance",
            &json!({
                "user_id": user_id,
                "memory_id": memory_id,
                "context": "created",
                "stance": "asserts",
                "certainty": 100,
                "linked_at": now
            }),
        )
        .await
        .expect("link Hive holder to memory");
    }

    let mut observed = 0i64;
    for _ in 0..20 {
        let result = db
            .execute_query::<CountResult, _>(
                "getContentKeyGroupUserCount",
                &json!({"content_key": content_key}),
            )
            .await
            .expect("read Hive fingerprint consensus");
        observed = observed.max(result.count);
        if observed == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    assert_eq!(
        observed, 2,
        "two exact author-level memories must project two Hive holders"
    );
    println!("==== hive_memory_e2e ==== exact fingerprint projects {observed} holders");
}
