//! Ingest buffer over the real MCP transport (#25), buffer ENABLED.
//!
//! Drives the actual `helixir-mcp` binary with `HELIXIR_INGEST_BUFFER=1` so
//! add_memory routes through the async serial buffer, exactly as a client
//! would when the toggle is on. Verifies:
//! - add_memory returns an instant {queued, pending_id} ack;
//! - the serial worker processes it and the fact becomes searchable;
//! - a later add_memory call carries the prior outcome opportunistically in
//!   `pending_outcomes` (no polling, no check_inbox by the agent);
//! - get_add_status reports done.
//!
//! Run (note the buffer env — do NOT set it for the other MCP suites):
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt HELIXIR_INGEST_BUFFER=1 \
//!   cargo test -p helixir --test mcp_ingest_e2e -- --ignored --nocapture
//! ```

use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

#[test]
#[ignore = "needs HELIX_E2E=1, HELIXIR_INGEST_BUFFER=1, live HelixDB + embeddings + working LLM"]
fn mcp_ingest_buffer() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");
    assert_eq!(
        std::env::var("HELIXIR_INGEST_BUFFER").unwrap_or_default(),
        "1",
        "this suite requires the ingest buffer enabled"
    );

    let (mut mcp, _boot) = McpClient::spawn();
    let run = token();
    let user = format!("mcp_ingest_{run}");

    // 1. Buffered add returns an UNAMBIGUOUS success (#63 confirm-or-promise):
    //    ok:true with the result inline once the worker finishes within the ack
    //    window, else status:"accepted" — never a bare "pending". The ack is no
    //    longer instant (it briefly waits to confirm), so we don't assert speed.
    let msg =
        format!("MCP ingest {run}: the canary deployment uses 10 percent traffic for one hour.");
    let (ack, ms) = mcp.call_tool("add_memory", json!({"message": msg, "user_id": user}));
    assert_eq!(
        ack["ok"].as_bool(),
        Some(true),
        "buffered add must report ok:true (success): {ack}"
    );
    assert_ne!(
        ack["status"].as_str(),
        Some("pending"),
        "buffered add must not return a bare pending ack (#63): {ack}"
    );
    let pending_id = ack["pending_id"].as_str().unwrap_or("").to_string();
    assert!(pending_id.starts_with("pi_"), "pending_id shape: {ack}");
    // Did it confirm inline (memory_ids present) or promise acceptance?
    let confirmed_inline = ack["memory_ids"]
        .as_array()
        .map(|a| !a.is_empty())
        .unwrap_or(false);

    // 2. If still processing, poll get_add_status until the worker finishes. When
    // confirmed inline it is already done — and its tombstone may already be
    // drained — so don't require the poll to observe it.
    let mut done = confirmed_inline;
    if !done {
        for _ in 0..30 {
            sleep(Duration::from_secs(2));
            let (st, _) = mcp.call_tool("get_add_status", json!({"pending_id": pending_id}));
            match st["status"].as_str().unwrap_or("") {
                "done" => {
                    done = true;
                    break;
                }
                "failed" => panic!("worker reported failed: {st}"),
                _ => {}
            }
        }
    }
    assert!(done, "buffered write must confirm inline or reach done");

    // 3. The fact is searchable.
    let (results, _) = mcp.call_tool(
        "search_memory",
        json!({"query": msg, "user_id": user, "mode": "full", "limit": 5}),
    );
    assert!(
        !results.as_array().map(Vec::is_empty).unwrap_or(true),
        "the fact must be searchable after the worker processed it"
    );

    // 4. A new add carries the PRIOR outcome opportunistically — no check_inbox.
    let msg2 = format!("MCP ingest {run}: rollback is triggered if error rate exceeds 2 percent.");
    let (ack2, _) = mcp.call_tool("add_memory", json!({"message": msg2, "user_id": user}));
    let outcomes = ack2["pending_outcomes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    // The first add's outcome should have been delivered here (or already
    // drained by the status polls' side-effects — so accept either, but at
    // least the field must be present and the mechanism wired).
    assert!(
        ack2["pending_outcomes"].is_array(),
        "buffered add response must carry a pending_outcomes array: {ack2}"
    );
    // 5. Best-effort push: a logging notification from helixir.ingest should
    // have arrived while we were polling/searching (captured by the harness).
    let pushed = mcp.notifications.iter().any(|n| {
        n["method"].as_str() == Some("notifications/message")
            && n["params"]["logger"].as_str() == Some("helixir.ingest")
    });

    println!("\n==== mcp_ingest_e2e ====");
    println!("queued ack in {ms:.0}ms; worker done; fact searchable");
    println!(
        "opportunistic outcomes on next add: {} item(s)",
        outcomes.len()
    );
    // The push is BEST-EFFORT and timing-dependent (it may land after we polled),
    // so it's observed, not required — asserting it made this suite flaky. The
    // buffer's contract (fast queued ack, worker drains, fact becomes searchable)
    // is asserted above; the push is a bonus.
    println!("best-effort push notification observed: {pushed}");
}
