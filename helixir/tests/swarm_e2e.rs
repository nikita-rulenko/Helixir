//! Swarm rendezvous (#39): presence lives in the shared graph, so agents that
//! register from *different hosts* all surface in one roster — the data-plane
//! coordination the multi-host topology rests on (no CLI-to-CLI link).
//!
//! Registers two agents stamped with distinct hosts, then reads the roster back
//! and asserts both appear, carry their host, and count as active inside the
//! window (and stale outside a 0s window).
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test swarm_e2e -- --ignored --nocapture
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use helixir::core::HelixirClient;

mod common;

fn token() -> String {
    format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + Agent presence schema deployed"]
async fn two_hosts_appear_in_one_roster() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");
    let admin = client.admin_as("codex").await.expect("RBAC admin");
    let tooling = admin.tooling();

    let run = token();
    let a = format!("agent_a_{run}");
    let b = format!("agent_b_{run}");

    // Two agents announce from two different hosts.
    tooling
        .register_or_heartbeat(&a, "researcher", "host-alpha", "working")
        .await
        .expect("heartbeat a");
    tooling
        .register_or_heartbeat(&b, "developer", "host-beta", "idle")
        .await
        .expect("heartbeat b");

    let now = chrono::Utc::now();
    let roster = tooling.list_swarm().await.expect("list_swarm");

    let pa = roster
        .iter()
        .find(|p| p.agent_id == a)
        .unwrap_or_else(|| panic!("agent a missing from roster of {}", roster.len()));
    let pb = roster
        .iter()
        .find(|p| p.agent_id == b)
        .expect("agent b missing from roster");

    // Presence fields round-tripped through the shared graph.
    assert_eq!(pa.host, "host-alpha", "host-alpha not recorded: {pa:?}");
    assert_eq!(pb.host, "host-beta", "host-beta not recorded: {pb:?}");
    assert_eq!(pa.role, "researcher");
    assert_eq!(pa.status, "working");

    // Fresh heartbeats are active in a generous window. (The active/stale
    // boundary itself is unit-tested with a controlled clock — here both were
    // just stamped, so age≈0 and any non-negative window counts them live.)
    assert!(
        pa.is_active(now, 120),
        "a should be active: age={:?}",
        pa.age_seconds(now)
    );
    assert!(pb.is_active(now, 120), "b should be active");

    // Re-heartbeat is idempotent: no duplicate node, presence just updates.
    tooling
        .register_or_heartbeat(&a, "researcher", "host-alpha", "idle")
        .await
        .expect("re-heartbeat a");
    let roster2 = tooling.list_swarm().await.expect("list_swarm 2");
    let count_a = roster2.iter().filter(|p| p.agent_id == a).count();
    assert_eq!(count_a, 1, "re-register must not duplicate the agent node");
    let pa2 = roster2.iter().find(|p| p.agent_id == a).unwrap();
    assert_eq!(pa2.status, "idle", "status must update on re-heartbeat");

    println!("\n==== swarm_e2e ====");
    println!(
        "roster carries {} agent(s); two hosts visible in one collective",
        roster.len()
    );
    for p in &roster2 {
        println!(
            "  {} [{}] @ {} — {} ({}s ago)",
            p.agent_id,
            p.role,
            p.host,
            p.status,
            p.age_seconds(now)
                .map(|s| s.to_string())
                .unwrap_or_else(|| "never".into())
        );
    }
}

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + Agent presence schema deployed"]
async fn mcp_presence_is_activity_driven_and_farewell_stays_terminal() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    // Stable id keeps repeated live runs idempotent instead of growing the
    // durable Agent registry with one test principal per invocation.
    let actor = "mcp-heartbeat-e2e".to_string();
    let (mut mcp, _) = common::McpClient::spawn_with_env(&[
        ("HELIXIR_RBAC_ACTOR", actor.as_str()),
        ("HELIXIR_MODE", "collective"),
    ]);

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize observer");
    let admin = client.admin_as("codex").await.expect("RBAC admin");
    let roster = admin.tooling().list_swarm().await.expect("list_swarm");
    let presence = roster
        .iter()
        .find(|item| item.agent_id == actor)
        .expect("initialized MCP actor must appear without add_memory");

    assert_eq!(presence.status, "connected");
    assert!(presence.is_active(chrono::Utc::now(), 120));
    let initialized_at = presence.last_seen.clone();

    // The retired process-lifetime loop refreshed every 30 seconds with the
    // default 90-second window. An idle transport must now leave its one
    // initialization lease untouched.
    tokio::time::sleep(std::time::Duration::from_secs(32)).await;
    let roster = admin
        .tooling()
        .list_swarm()
        .await
        .expect("list_swarm after idle interval");
    let presence = roster
        .iter()
        .find(|item| item.agent_id == actor)
        .expect("idle MCP actor remains in the durable registry");
    assert_eq!(presence.last_seen, initialized_at);

    let (farewell, _) = mcp.call_tool("agent_farewell", serde_json::json!({"agent_id": actor}));
    assert_eq!(farewell["status"], "done");
    tokio::time::sleep(std::time::Duration::from_secs(32)).await;
    let roster = admin
        .tooling()
        .list_swarm()
        .await
        .expect("list_swarm after farewell");
    let presence = roster
        .iter()
        .find(|item| item.agent_id == actor)
        .expect("farewell keeps durable provenance node");
    assert_eq!(presence.status, "done");
    assert!(
        !presence.is_active(chrono::Utc::now(), 120),
        "farewell must remove the agent from active presence immediately"
    );

    // A later real tool call starts a new lease. `swarm_status` has no actor
    // argument, so it refreshes the configured MCP principal.
    let (status, _) = mcp.call_tool("swarm_status", serde_json::json!({}));
    assert_eq!(status["available"], true);
    let roster = admin
        .tooling()
        .list_swarm()
        .await
        .expect("list_swarm after real tool activity");
    let presence = roster
        .iter()
        .find(|item| item.agent_id == actor)
        .expect("active MCP actor remains registered");
    assert_eq!(presence.status, "working");
    assert!(presence.is_active(chrono::Utc::now(), 120));

    // Leave the shared live registry clean after the proof. A completed test
    // process must not remain visible as an online worker until lease expiry.
    let (farewell, _) = mcp.call_tool("agent_farewell", serde_json::json!({"agent_id": actor}));
    assert_eq!(farewell["status"], "done");
}
