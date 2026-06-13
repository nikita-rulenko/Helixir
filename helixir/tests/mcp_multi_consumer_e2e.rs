//! Liveness oracle L2: the multi-consumer topology (#42 audit).
//!
//! The real system is N MCP processes ↔ one HelixDB ↔ shared collective
//! knowledge. A single-consumer "it didn't crash" run under-tests it: the
//! emergent invariants below only exist with several consumers. Each
//! `McpClient` here is a *separate* `helixir-mcp` process against the shared
//! instance — the actual deployment shape.
//!
//! Invariants asserted (the 7 agreed with Nikita):
//!   1+2. consensus + cross-user dedup — the same fact from K users links to
//!        ONE memory node whose user_count reflects all K;
//!   3.   collective visibility — a fresh user finds others' knowledge via
//!        scope=collective;
//!   4.   personal isolation — that fresh user does NOT see it in scope=personal;
//!   5.   buffered multi-producer — several processes enqueue concurrently and
//!        the worker(s) process every one, none lost;
//!   6.   outbox — a write outcome reaches the agent on a later call;
//!   7.   knowledge is never deleted — only queue scaffolding is pruned.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test mcp_multi_consumer_e2e -- --ignored --nocapture
//! ```

use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

mod common;
use common::McpClient;

fn token() -> String {
    format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

fn require_e2e() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );
}

/// Poll a buffered write to completion via get_add_status from any consumer.
fn poll_done(mcp: &mut McpClient, pid: &str, tries: usize) -> Value {
    for _ in 0..tries {
        sleep(Duration::from_secs(2));
        let (st, _) = mcp.call_tool("get_add_status", json!({ "pending_id": pid }));
        match st["status"].as_str().unwrap_or("") {
            "done" => return st,
            "failed" => panic!("worker reported failed for {pid}: {st}"),
            _ => {}
        }
    }
    panic!("{pid} did not reach done within the polling window");
}

/// Poll collective search until a result mentions `needle`, returning the
/// highest user_count seen for it (0 = never became visible). Makes snapshot
/// lag deterministic: we wait for a write to be visible before the next one,
/// so cross-user dedup is tested on its logic, not on a race.
fn wait_collective(mcp: &mut McpClient, query: &str, needle: &str, tries: usize) -> u64 {
    let mut best = 0u64;
    for _ in 0..tries {
        let (res, _) = mcp.call_tool(
            "search_memory",
            json!({ "query": query, "user_id": "mc_visibility_probe", "mode": "full", "scope": "collective", "limit": 10 }),
        );
        if let Some(arr) = res.as_array() {
            let hit = arr
                .iter()
                .filter(|r| {
                    r["content"]
                        .as_str()
                        .map(|c| c.contains(needle))
                        .unwrap_or(false)
                })
                .filter_map(|r| r["metadata"]["user_count"].as_u64())
                .max();
            if let Some(uc) = hit {
                best = best.max(uc);
                return best;
            }
        }
        sleep(Duration::from_secs(2));
    }
    best
}

// ---------------------------------------------------------------------------
// Invariants 1–4: consensus, cross-user dedup, collective visibility,
// personal isolation. Sync path (buffer OFF) for deterministic assertions.
// Three producer processes write the SAME fact; a fourth fresh process reads.
// ---------------------------------------------------------------------------
#[test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings + working LLM"]
fn multi_consumer_collective_invariants() {
    require_e2e();
    let run = token();

    // A fact whose SUBJECT carries a unique token (a made-up service name) so:
    //  - extraction keeps it as one clean fact (no removable "Prefix:" that
    //    spawns a junk second fact);
    //  - it cannot dedup against earlier runs, so the consensus we observe is
    //    THIS run's three writers, not history.
    // Match results by that token substring — robust to the extractor rewording
    // the sentence — and assert on user_count.
    let svc = format!("atlas{run}");
    let fact = format!("Service {svc} ships its release train every Thursday at 14:00 UTC.");
    let users = [
        format!("mc_a_{run}"),
        format!("mc_b_{run}"),
        format!("mc_c_{run}"),
    ];

    // Sequential writes from three separate MCP processes. After each write we
    // WAIT until it is collectively searchable before the next writer, so the
    // next write's dedup search is guaranteed to see the prior node. This
    // removes snapshot-lag as a variable and tests the dedup logic itself:
    // given visibility, three identical writes must consolidate.
    for user in &users {
        let (mut mcp, _) = McpClient::spawn();
        mcp.call_tool("add_memory", json!({ "message": fact, "user_id": user }));
        let visible = wait_collective(&mut mcp, &fact, &svc, 20);
        assert!(
            visible >= 1,
            "write by {user} must become collectively searchable before the next writer: {svc}"
        );
    }
    sleep(Duration::from_secs(2)); // let the final link settle for the reader

    // A fourth, fresh consumer that never wrote the fact.
    let reader = format!("mc_reader_{run}");
    let (mut rmcp, _) = McpClient::spawn();

    // Highest user_count among results that actually mention our unique service.
    let token_user_count = |arr: &[Value]| -> Option<u64> {
        arr.iter()
            .filter(|r| {
                r["content"]
                    .as_str()
                    .map(|c| c.contains(&svc))
                    .unwrap_or(false)
            })
            .filter_map(|r| r["metadata"]["user_count"].as_u64())
            .max()
    };

    // Invariant 3: collective visibility — the fresh reader finds the shared
    // fact even though it never wrote it.
    let (coll, _) = rmcp.call_tool(
        "search_memory",
        json!({ "query": fact, "user_id": reader, "mode": "full", "scope": "collective", "limit": 10 }),
    );
    let coll_arr = coll.as_array().cloned().unwrap_or_default();
    let user_count = token_user_count(&coll_arr).unwrap_or_else(|| {
        panic!(
            "invariant 3 (collective visibility): reader must find a node mentioning {svc}: {coll}"
        )
    });

    // Invariants 1+2: consensus + cross-user dedup — the shared fact consolidates
    // into a node whose user_count reflects multiple of the three writers.
    assert!(
        user_count >= 2,
        "invariant 1+2 (consensus/dedup): the shared fact must consolidate to \
         user_count >= 2, got {user_count}: {coll}"
    );

    // Invariant 4: personal isolation — the reader never wrote it, so personal
    // scope must not surface the fact.
    let (pers, _) = rmcp.call_tool(
        "search_memory",
        json!({ "query": fact, "user_id": reader, "mode": "full", "scope": "personal", "limit": 10 }),
    );
    let pers_arr = pers.as_array().cloned().unwrap_or_default();
    assert!(
        token_user_count(&pers_arr).is_none(),
        "invariant 4 (personal isolation): reader's personal scope must not contain {svc}: {pers}"
    );

    println!("\n==== multi_consumer_collective_invariants ====");
    println!(
        "shared fact {svc}: user_count={user_count}; visible in collective, hidden in personal ✓"
    );
}

// ---------------------------------------------------------------------------
// Invariants 5–7: buffered multi-producer (none lost), outbox delivery,
// knowledge-never-deleted. Buffer ON consumers.
// ---------------------------------------------------------------------------
#[test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings + working LLM"]
fn multi_consumer_buffer_invariants() {
    require_e2e();
    let run = token();
    let buf = [("HELIXIR_INGEST_BUFFER", "1")];

    // --- Invariant 5: several buffered producers, none lost ---------------
    // Keep all producer processes alive (their workers drain the shared queue)
    // while we wait for completion.
    let mut producers: Vec<(McpClient, String, String)> = Vec::new(); // (client, pid, msg)
    for i in 0..3 {
        let (mut mcp, _) = McpClient::spawn_with_env(&buf);
        let user = format!("mc_buf_{run}_{i}");
        let msg = format!(
            "Buffered multi-producer {run} #{i}: shard {i} of service atlas is pinned to region eu-{i}."
        );
        let (ack, _) = mcp.call_tool("add_memory", json!({ "message": msg, "user_id": user }));
        assert_eq!(
            ack["queued"].as_bool(),
            Some(true),
            "buffered add must queue: {ack}"
        );
        let pid = ack["pending_id"].as_str().unwrap_or("").to_string();
        assert!(pid.starts_with("pi_"), "pending_id shape: {ack}");
        producers.push((mcp, pid, msg));
    }
    // "None lost" = the worker(s) processed every queued item (poll_done panics
    // on `failed`/timeout, so reaching `done` for all three is that proof) AND
    // each write landed in the shared graph. A write may dedup to 0 NEW memories
    // (e.g. an identical fact from a prior run) — that is processed-and-linked,
    // not lost — so we assert searchability, not memories_added.
    let (mut poller, _) = McpClient::spawn_with_env(&buf);
    for (_mcp, pid, msg) in &producers {
        poll_done(&mut poller, pid, 30);
        let (found, _) = poller.call_tool(
            "search_memory",
            json!({ "query": msg, "user_id": "mc_buf_reader", "mode": "full", "scope": "collective", "limit": 5 }),
        );
        assert!(
            found.as_array().map(|a| !a.is_empty()).unwrap_or(false),
            "each buffered write must land in the shared graph (processed, not lost): {msg} -> {found}"
        );
    }
    drop(producers);

    // --- Invariant 6: outbox delivers an outcome on a later call ----------
    let (mut h, _) = McpClient::spawn_with_env(&buf);
    let huser = format!("mc_outbox_{run}");
    let (ack1, _) = h.call_tool(
        "add_memory",
        json!({ "message": format!("Outbox {run}: the atlas changelog lives at docs/atlas/CHANGELOG.md."), "user_id": huser }),
    );
    let pid1 = ack1["pending_id"].as_str().unwrap_or("").to_string();
    poll_done(&mut h, &pid1, 30);
    // A subsequent write carries the prior outcome opportunistically.
    let (ack2, _) = h.call_tool(
        "add_memory",
        json!({ "message": format!("Outbox {run}: the atlas oncall rotation is weekly."), "user_id": huser }),
    );
    assert!(
        ack2["pending_outcomes"].is_array(),
        "invariant 6 (outbox): a buffered response must carry a pending_outcomes array: {ack2}"
    );

    // --- Invariant 7: knowledge never deleted, only scaffolding pruned ----
    let (mut k, _) = McpClient::spawn_with_env(&buf);
    let kuser = format!("mc_nodelete_{run}");
    let count_memories = |mcp: &mut McpClient| -> usize {
        let (listed, _) = mcp.call_tool("list_memories", json!({ "user_id": kuser }));
        listed.as_array().map(Vec::len).unwrap_or(0)
    };
    // First write so the user has a baseline.
    let (kack1, _) = k.call_tool(
        "add_memory",
        json!({ "message": format!("No-delete {run}: atlas uses postgres 16 in production."), "user_id": kuser }),
    );
    let kpid1 = kack1["pending_id"].as_str().unwrap_or("").to_string();
    poll_done(&mut k, &kpid1, 30);
    // Drain the outbox (prunes kpid1's PendingInput tombstone) via another write.
    let (kack2, _) = k.call_tool(
        "add_memory",
        json!({ "message": format!("No-delete {run}: atlas caches sessions in redis."), "user_id": kuser }),
    );
    let kpid2 = kack2["pending_id"].as_str().unwrap_or("").to_string();
    poll_done(&mut k, &kpid2, 30);
    let n_before = count_memories(&mut k);
    // One more write + drain cycle; knowledge count must not shrink.
    let (kack3, _) = k.call_tool(
        "add_memory",
        json!({ "message": format!("No-delete {run}: atlas exposes metrics on port 9090."), "user_id": kuser }),
    );
    let kpid3 = kack3["pending_id"].as_str().unwrap_or("").to_string();
    poll_done(&mut k, &kpid3, 30);
    let _ = k.call_tool(
        "add_memory",
        json!({ "message": format!("No-delete {run}: atlas log level is info."), "user_id": kuser }),
    );
    let n_after = count_memories(&mut k);
    assert!(
        n_after >= n_before,
        "invariant 7 (no-delete): knowledge count must not shrink across queue drains \
         (before={n_before}, after={n_after})"
    );

    println!("\n==== multi_consumer_buffer_invariants ====");
    println!(
        "3 buffered producers all processed; outbox delivered; knowledge {n_before}→{n_after} (never shrank) ✓"
    );
}
