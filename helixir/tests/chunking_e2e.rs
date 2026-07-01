//! #9 guard: content-chunking on the LIVE write path. A long single-topic memory
//! (> the 500-char threshold) must be split into MemoryChunk nodes via
//! add_memory_with_chunking. This path was broken — it called a nonexistent
//! query `addChunk` (NotFound), so chunks_created stayed 0 — until the call was
//! fixed to `addMemoryChunk` (field parent_memory_id).
//!
//! ```text
//! HELIX_E2E=1 cargo test -p helixir --test chunking_e2e -- --ignored --nocapture
//! ```

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

#[test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings + working LLM"]
fn long_memory_is_chunked_on_write_9() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );
    assert_ne!(
        std::env::var("HELIXIR_INGEST_BUFFER").unwrap_or_default(),
        "1",
        "this test runs the synchronous path — do NOT set HELIXIR_INGEST_BUFFER"
    );

    let (mut mcp, _boot) = McpClient::spawn();
    let run = token();
    let user = format!("chunk9_{run}");

    // One long, COHERENT passage so the extractor keeps it as a single atom that
    // exceeds the 500-char chunk threshold (multi-topic input would be shattered
    // into short atoms that never chunk).
    let long = format!(
        "Chunking probe {run}: this is a single deliberately long and coherent passage about one \
         topic so the extractor does not shatter it into many tiny atoms. It describes, at length \
         and without changing subject, how the Helixir content-chunking subsystem is supposed to \
         take any stored memory whose text exceeds the configured character threshold and split it \
         into a sequence of overlapping MemoryChunk nodes, each linked back to its parent memory by \
         a HAS_CHUNK edge and embedded for sub-document semantic retrieval, so that very long notes \
         remain searchable at the granularity of their individual passages rather than only as one \
         opaque blob, which is the whole point of having a chunking stage in the write pipeline at \
         all, and this sentence keeps going to make absolutely sure the stored atom clears the \
         five-hundred-character threshold that gates the should_chunk decision in the live path."
    );

    let (added, _) = mcp.call_tool("add_memory", json!({"message": long, "user_id": user}));
    assert!(added["ok"].as_bool().unwrap_or(false), "write ok: {added}");

    // The fix's proof: a memory long enough to cross the threshold yields chunks.
    let chunks_created = added["chunks_created"].as_u64().unwrap_or(0);
    assert!(
        chunks_created >= 1,
        "a >500-char memory must be chunked on write (add_memory_with_chunking → \
         addMemoryChunk); got chunks_created={chunks_created}: {added}"
    );

    println!("\n==== long_memory_is_chunked_on_write_9 ====");
    println!("long memory chunked on write: chunks_created={chunks_created} ✓");
}
