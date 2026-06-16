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

fn token() -> String {
    format!(
        "{:x}",
        SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos()
    )
}

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + Agent presence schema deployed"]
async fn two_hosts_appear_in_one_roster() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");
    let tooling = client.tooling();

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

    // Fresh heartbeats are active in a generous window, stale in a zero window.
    assert!(pa.is_active(now, 120), "a should be active: age={:?}", pa.age_seconds(now));
    assert!(pb.is_active(now, 120), "b should be active");
    assert!(!pa.is_active(now, 0), "a should be stale at window=0");

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
    println!("roster carries {} agent(s); two hosts visible in one collective", roster.len());
    for p in &roster2 {
        println!(
            "  {} [{}] @ {} — {} ({}s ago)",
            p.agent_id,
            p.role,
            p.host,
            p.status,
            p.age_seconds(now).map(|s| s.to_string()).unwrap_or_else(|| "never".into())
        );
    }
}
