//! Ingest buffer end-to-end (#25): the bidirectional persistent buffer.
//!
//! Verifies the agreed design empirically (the methodology HQL forces:
//! docs → helix check → autotest the actual behaviour):
//! - enqueue returns a pending_id instantly without running the pipeline;
//! - the serial drain processes the queued input through the real pipeline;
//! - the outcome lands in the user's outbox (прихожая);
//! - draining the outbox delivers the result, marks it delivered, and prunes
//!   the queue tombstone (PendingInput);
//! - the resulting memory is searchable.
//!
//! Requires live HelixDB + embeddings + a working LLM:
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test ingest_buffer_e2e -- --ignored --nocapture
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use helixir::core::HelixirClient;

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
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings + working LLM"]
async fn ingest_buffer_roundtrip() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );

    let client = HelixirClient::from_env().expect("HelixirClient::from_env");
    client.initialize().await.expect("initialize");

    let run = token();
    let user = format!("ingest_e2e_{run}");
    let msg = format!(
        "Ingest buffer e2e {run}: the canonical deployment region is eu-west-3 \
         and the on-call rotation is weekly."
    );

    // 1. Enqueue — must return a pending_id without running the pipeline.
    let enq = client
        .add_buffered(&msg, &user, None, None)
        .await
        .expect("add_buffered");
    assert!(enq.queued, "buffered add must report queued=true");
    assert!(
        enq.pending_id.starts_with("pi_"),
        "pending_id shape: {}",
        enq.pending_id
    );
    assert_eq!(enq.status, "pending");

    // The pipeline has NOT run yet — nothing is searchable.
    let before = client
        .search(
            &msg,
            &user,
            helixir::core::helixir_client::SearchParams {
                limit: Some(5),
                search_mode: Some("full".to_string()),
                scope: Some("personal".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("search before");
    assert!(
        before.is_empty(),
        "the fact must not be searchable before the worker processes it"
    );

    // 2. Drain the queue serially (what the background worker does).
    let processed = client.tooling().drain_pending_once().await;
    assert!(processed >= 1, "drain must process the queued item");

    // 3. Status is now done, with a result payload.
    let status = client
        .add_status(&enq.pending_id)
        .await
        .expect("add_status");
    assert_eq!(status.status, "done", "status after drain: {status:?}");
    let result = status.result.expect("done status must carry a result");
    assert!(
        result["memories_added"].as_u64().unwrap_or(0) >= 1,
        "result must report stored memories: {result}"
    );

    // 4. The fact is now searchable.
    let after = client
        .search(
            &msg,
            &user,
            helixir::core::helixir_client::SearchParams {
                limit: Some(5),
                search_mode: Some("full".to_string()),
                scope: Some("personal".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("search after");
    assert!(
        !after.is_empty(),
        "the fact must be searchable after the worker processed it"
    );

    // 5. Outbox carries the outcome; draining it delivers and prunes.
    let notices = client
        .drain_notices(&user, 20)
        .await
        .expect("drain_notices");
    assert!(
        notices
            .iter()
            .any(|n| n.kind == "add_result" && n.pending_id == enq.pending_id),
        "outbox must carry an add_result for this pending_id: {notices:?}"
    );

    // Idempotent drain: the delivered notice does not come back, and the
    // tombstone PendingInput was pruned (status now not_found).
    let second = client
        .drain_notices(&user, 20)
        .await
        .expect("drain_notices 2");
    assert!(
        !second.iter().any(|n| n.pending_id == enq.pending_id),
        "delivered notices must not be redelivered"
    );
    let gone = client
        .add_status(&enq.pending_id)
        .await
        .expect("add_status gone");
    assert_eq!(
        gone.status, "not_found",
        "the queue tombstone must be pruned after delivery"
    );

    println!("\n==== ingest_buffer_e2e ====");
    println!("pending_id {} processed; result {result}", enq.pending_id);
    println!("outbox delivered + tombstone pruned; fact searchable");
}
