//! Concurrent multi-agent MCP stress: collective/all under parallel load.
//!
//! Spawns N independent `helixir-mcp` processes, each a distinct `user_id`,
//! and has them hammer `search_memory` with `scope=collective` / `scope=all`
//! in parallel — several agents driving the heavy ranking path against one
//! shared HelixDB at once. The harness turns a server death into a signal: a
//! crashed child closes its stdout, `McpClient::request` panics
//! ("helixir-mcp closed stdout"), and the worker thread fails its `join()`.
//!
//! Honest scope note (#41 / #42): this is a regression guard for "concurrent
//! collective/all does not crash the server", NOT a reproducer of the live
//! zeroclaw crash. It passes on BOTH the pre-fix and post-fix binary (a NaN
//! is in fact unreachable through the normal pipeline — see #41), so the NaN
//! ranking fix is latent-bug hardening, not the crash cause. The real crash
//! suspect is the multi-process model (per-client warmup/self-seed → resource
//! multiplication), tracked in the memory-provider epic #42. The NaN→no-panic
//! logic itself is proven directly by the unit tests in `mind_toolbox::ranking`.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test concurrent_mcp_stress_e2e -- --ignored --nocapture
//! ```

use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

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

const AGENTS: usize = 5;
const SEARCHES_PER_AGENT: usize = 10;

#[test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings + working LLM"]
fn concurrent_collective_search_does_not_crash() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );

    let run = token();

    // Varied queries hitting the full ranking path. "a" is deliberately
    // degenerate — the single most likely input to surface an odd score in
    // the vector/rerank/graph blend that the sorts then order.
    let queries = [
        "deployment region architecture decision",
        "ingest buffer serial worker dedup",
        "a",
        "clean architecture repository interfaces",
        "memory charter rules escalation",
    ];

    let handles: Vec<thread::JoinHandle<usize>> = (0..AGENTS)
        .map(|i| {
            let run = run.clone();
            let queries = queries;
            thread::spawn(move || {
                // Each agent is its own MCP process — independent stdio child,
                // exactly like separate clients (zeroclaw + a Claude session).
                let (mut mcp, _boot) = McpClient::spawn();
                let user = format!("stress_{run}_{i}");

                let mut ok = 0usize;
                for s in 0..SEARCHES_PER_AGENT {
                    let q = queries[(i + s) % queries.len()];
                    // Alternate the two scopes that fan out across all users.
                    let scope = if s % 2 == 0 { "collective" } else { "all" };
                    // If the child crashed, request() panics here on the closed
                    // pipe — failing this thread, which the parent detects.
                    let (_payload, _ms) = mcp.call_tool(
                        "search_memory",
                        json!({
                            "query": q,
                            "user_id": user,
                            "mode": "full",
                            "scope": scope,
                            "limit": 8
                        }),
                    );
                    ok += 1;
                }
                ok
            })
        })
        .collect();

    let mut total = 0usize;
    let mut crashed = 0usize;
    for (i, h) in handles.into_iter().enumerate() {
        match h.join() {
            Ok(n) => total += n,
            Err(_) => {
                crashed += 1;
                eprintln!("agent {i}: worker thread panicked — its MCP child likely crashed");
            }
        }
    }

    println!("\n==== concurrent_mcp_stress_e2e ====");
    println!(
        "{AGENTS} concurrent agents x {SEARCHES_PER_AGENT} collective/all searches: \
         {total} succeeded, {crashed} crashed"
    );
    assert_eq!(
        crashed, 0,
        "every MCP process must survive concurrent collective/all load (#41)"
    );
    assert_eq!(
        total,
        AGENTS * SEARCHES_PER_AGENT,
        "all concurrent searches must complete"
    );
}
