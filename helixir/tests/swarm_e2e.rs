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
use helixir::toolkit::tooling_manager::swarm::aggregate_agent_families;

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
    let principal = format!("family_{run}");
    let children = [
        format!("{principal}-build"),
        format!("{principal}-review"),
        format!("{principal}-research"),
    ];

    tooling
        .register_or_heartbeat_as(&principal, &principal, "developer", "host-root", "working")
        .await
        .expect("heartbeat root");
    for (child, host) in children
        .iter()
        .zip(["host-alpha", "host-beta", "host-gamma"])
    {
        tooling
            .register_or_heartbeat_as(child, &principal, "developer", host, "working")
            .await
            .expect("heartbeat child");
    }

    // Model a root process that crashed without farewell while three child
    // leases remain fresh. The family must count as one logical agent and
    // stay online through its children.
    admin
        .db()
        .execute_query::<serde_json::Value, _>(
            "heartbeatAgent",
            &serde_json::json!({
                "agent_id": &principal,
                "principal_id": &principal,
                "host": "host-root",
                "last_seen": (chrono::Utc::now() - chrono::Duration::minutes(10)).to_rfc3339(),
                "status": "working",
            }),
        )
        .await
        .expect("age root lease");

    let now = chrono::Utc::now();
    let roster = tooling.list_swarm().await.expect("list_swarm");
    let instances = roster
        .iter()
        .filter(|presence| presence.principal_id == principal)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(instances.len(), 4, "one root plus three child instances");
    let root = instances
        .iter()
        .find(|presence| presence.agent_id == principal)
        .expect("root presence");
    assert!(!root.is_active(now, 120), "aged root must be stale");
    let families = aggregate_agent_families(&instances, now, 120);
    assert_eq!(families.len(), 1);
    assert_eq!(families[0].principal_id, principal);
    assert_eq!(families[0].total_instances, 4);
    assert_eq!(families[0].active_instances, 3);
    assert!(families[0].active);

    // Farewell is instance-scoped: one child leaves while its two siblings
    // keep the logical family online.
    tooling
        .register_or_heartbeat_as(&children[0], &principal, "developer", "host-alpha", "done")
        .await
        .expect("farewell first child");
    let final_roster = tooling.list_swarm().await.expect("final roster");
    let final_instances = final_roster
        .iter()
        .filter(|presence| presence.principal_id == principal)
        .cloned()
        .collect::<Vec<_>>();
    let families = aggregate_agent_families(&final_instances, chrono::Utc::now(), 120);
    assert_eq!(families[0].active_instances, 2);
    assert!(families[0].active, "active siblings keep family online");

    tooling
        .register_or_heartbeat_as(&principal, &principal, "developer", "host-root", "done")
        .await
        .expect("cleanup root");
    for (child, host) in children.iter().skip(1).zip(["host-beta", "host-gamma"]) {
        tooling
            .register_or_heartbeat_as(child, &principal, "developer", host, "done")
            .await
            .expect("cleanup child");
    }

    let contested = format!("contested_{run}");
    let (claim_a, claim_b) = tokio::join!(
        tooling.register_or_heartbeat_as(
            &contested,
            "principal-a",
            "developer",
            "host-a",
            "working",
        ),
        tooling.register_or_heartbeat_as(
            &contested,
            "principal-b",
            "developer",
            "host-b",
            "working",
        ),
    );
    assert_ne!(
        claim_a.is_ok(),
        claim_b.is_ok(),
        "exactly one concurrent principal may claim an instance: a={claim_a:?} b={claim_b:?}"
    );
    let owner = tooling
        .list_swarm()
        .await
        .expect("contested roster")
        .into_iter()
        .find(|presence| presence.agent_id == contested)
        .expect("one contested owner")
        .principal_id;
    assert!(owner == "principal-a" || owner == "principal-b");
    tooling
        .register_or_heartbeat_as(&contested, &owner, "developer", "winner", "done")
        .await
        .expect("cleanup contested owner");

    println!("\n==== swarm_e2e ====");
    println!(
        "roster carries {} agent(s); one stale root and three children share one family",
        roster.len()
    );
    for p in &instances {
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
    // Transport initialization and passive status reads are not agent leases.
    // Establish a terminal row, then prove swarm_status does not resurrect it.
    admin
        .tooling()
        .register_or_heartbeat_as(&actor, &actor, "developer", "observer", "working")
        .await
        .expect("establish observer row");
    assert!(
        admin
            .tooling()
            .farewell_existing(&actor, &actor, "developer", "observer")
            .await
            .expect("terminal observer")
    );
    let before_status = admin
        .tooling()
        .list_swarm()
        .await
        .expect("list before passive status")
        .into_iter()
        .find(|item| item.agent_id == actor)
        .expect("observer row")
        .last_seen;
    let (passive_status, _) = mcp.call_tool("swarm_status", serde_json::json!({}));
    assert_eq!(passive_status["available"], true);
    let observer = admin
        .tooling()
        .list_swarm()
        .await
        .expect("list after passive status")
        .into_iter()
        .find(|item| item.agent_id == actor)
        .expect("observer row remains");
    assert_eq!(observer.status, "done");
    assert_eq!(observer.last_seen, before_status);

    // Exercise the exact logical-family contract through MCP without letting
    // swarm_status refresh the target root (the configured MCP actor is a
    // separate observer). One stale root and three live children must remain
    // one online logical agent with three active subagents.
    let target = "mcp-family-contract-e2e";
    let children = [
        "mcp-family-contract-e2e-build",
        "mcp-family-contract-e2e-review",
        "mcp-family-contract-e2e-research",
    ];
    let (enrollment, _) = mcp.call_tool("enroll_client", serde_json::json!({"actor_id": target}));
    assert_eq!(enrollment["principal_id"], target);
    admin
        .tooling()
        .register_or_heartbeat_as(target, target, "developer", "host-root", "working")
        .await
        .expect("target root heartbeat");
    admin
        .db()
        .execute_query::<serde_json::Value, _>(
            "heartbeatAgent",
            &serde_json::json!({
                "agent_id": target,
                "principal_id": target,
                "host": "host-root",
                "last_seen": (chrono::Utc::now() - chrono::Duration::minutes(10)).to_rfc3339(),
                "status": "working",
            }),
        )
        .await
        .expect("age target root lease");
    for child in children {
        let (heartbeat, _) = mcp.call_tool(
            "agent_heartbeat",
            serde_json::json!({
                "actor_id": target,
                "agent_id": child,
                "status": "testing family presence",
            }),
        );
        assert_eq!(heartbeat["available"], true);
        assert_eq!(heartbeat["principal_id"], target);
    }
    let (listed, _) = mcp.call_tool(
        "list_memories",
        serde_json::json!({"actor_id": target, "user_id": target, "limit": 1}),
    );
    assert!(
        listed.is_array(),
        "ordinary reads remain available: {listed}"
    );
    let (family_status, _) = mcp.call_tool("swarm_status", serde_json::json!({}));
    let family = family_status["families"]
        .as_array()
        .and_then(|families| {
            families
                .iter()
                .find(|family| family["principal_id"].as_str() == Some(target))
        })
        .expect("target logical family");
    assert_eq!(family["total_instances"], 4);
    assert_eq!(family["active_instances"], 3);
    assert_eq!(family["active"], true);
    let target_subagents = family_status["subagents"]
        .as_array()
        .expect("subagent roster")
        .iter()
        .filter(|instance| instance["principal_id"].as_str() == Some(target))
        .count();
    assert_eq!(target_subagents, 3);

    let (child_farewell, _) = mcp.call_tool(
        "agent_farewell",
        serde_json::json!({"actor_id": target, "agent_id": children[0]}),
    );
    assert_eq!(child_farewell["status"], "done");
    let (after_child, _) = mcp.call_tool("swarm_status", serde_json::json!({}));
    let family = after_child["families"]
        .as_array()
        .and_then(|families| {
            families
                .iter()
                .find(|family| family["principal_id"].as_str() == Some(target))
        })
        .expect("siblings keep target family visible");
    assert_eq!(family["active"], true);
    assert_eq!(family["active_instances"], 2);

    let (target_root_farewell, _) = mcp.call_tool(
        "agent_farewell",
        serde_json::json!({"actor_id": target, "agent_id": target}),
    );
    assert_eq!(target_root_farewell["status"], "done");
    for child in children.iter().skip(1) {
        let (cleanup, _) = mcp.call_tool(
            "agent_farewell",
            serde_json::json!({"actor_id": target, "agent_id": child}),
        );
        assert_eq!(cleanup["status"], "done");
    }

    let unknown = format!("never-announced-{}", token());
    let (unknown_farewell, _) = mcp.call_tool(
        "agent_farewell",
        serde_json::json!({"actor_id": target, "agent_id": unknown}),
    );
    assert_eq!(unknown_farewell["found"], false);
    assert_eq!(unknown_farewell["status"], "not_found");
    assert!(
        admin
            .tooling()
            .list_swarm()
            .await
            .expect("roster after unknown farewell")
            .iter()
            .all(|presence| presence.agent_id != unknown),
        "farewell must not create a durable row for an unknown instance"
    );

    let foreign_instance = format!("foreign-instance-{}", token());
    admin
        .tooling()
        .register_or_heartbeat_as(
            &foreign_instance,
            "another-principal",
            "developer",
            "foreign-host",
            "working",
        )
        .await
        .expect("register foreign instance");
    let ownership_error = mcp.call_tool_expect_error(
        "add_memory",
        serde_json::json!({
            "actor_id": actor,
            "user_id": actor,
            "agent_id": foreign_instance,
            "message": "This must fail before extraction or persistence.",
            "group_id": "default",
        }),
    );
    assert!(
        ownership_error.contains("already belongs to principal 'another-principal'"),
        "unexpected ownership error: {ownership_error}"
    );
    let farewell_error = mcp.call_tool_expect_error(
        "agent_farewell",
        serde_json::json!({"actor_id": target, "agent_id": foreign_instance}),
    );
    assert!(
        farewell_error.contains("belongs to principal 'another-principal'"),
        "unexpected cross-principal farewell error: {farewell_error}"
    );
    let foreign = admin
        .tooling()
        .list_swarm()
        .await
        .expect("roster after rejected ownership")
        .into_iter()
        .find(|presence| presence.agent_id == foreign_instance)
        .expect("foreign instance remains registered");
    assert_eq!(foreign.principal_id, "another-principal");
    admin
        .tooling()
        .register_or_heartbeat_as(
            &foreign_instance,
            "another-principal",
            "developer",
            "foreign-host",
            "done",
        )
        .await
        .expect("cleanup foreign instance");

    let (farewell, _) = mcp.call_tool(
        "agent_farewell",
        serde_json::json!({"actor_id": actor, "agent_id": actor}),
    );
    assert_eq!(farewell["status"], "done");
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

    // Passive reads do not create a lease. Only an explicit heartbeat starts
    // a new one, and farewell terminates exactly that owned instance.
    let (status, _) = mcp.call_tool("swarm_status", serde_json::json!({}));
    assert_eq!(status["available"], true);
    let passive = admin
        .tooling()
        .list_swarm()
        .await
        .expect("list_swarm after passive status")
        .into_iter()
        .find(|item| item.agent_id == actor)
        .expect("terminal actor remains registered");
    assert_eq!(passive.status, "done");
    let (heartbeat, _) = mcp.call_tool(
        "agent_heartbeat",
        serde_json::json!({"actor_id": actor, "agent_id": actor, "status": "working"}),
    );
    assert_eq!(heartbeat["status"], "working");
    let roster = admin.tooling().list_swarm().await.expect("active roster");
    let presence = roster
        .iter()
        .find(|item| item.agent_id == actor)
        .expect("active MCP actor remains registered");
    assert_eq!(presence.status, "working");
    assert!(presence.is_active(chrono::Utc::now(), 120));

    // Leave the shared live registry clean after the proof. A completed test
    // process must not remain visible as an online worker until lease expiry.
    let (farewell, _) = mcp.call_tool(
        "agent_farewell",
        serde_json::json!({"actor_id": actor, "agent_id": actor}),
    );
    assert_eq!(farewell["status"], "done");
}
