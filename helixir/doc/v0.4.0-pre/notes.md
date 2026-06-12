# v0.4.0-pre — local-first reasoning memory (pre-release on `dev`)

> _Snapshot of `dev` @ f3aa848, 2026-06-12. Frozen after the
> local-reasoning merge; do not edit._

Everything below is gated by `HELIXIR_RETRIEVAL_PROFILE=algo_opt`;
the default `legacy` profile preserves v0.3.1-fix behaviour bit-for-bit.

## Read path

- **Hybrid retrieval**: dense ANN + HelixDB-native `SearchBM25`, fused by
  Reciprocal Rank Fusion (k=60). Temporal cutoff pushed into HQL with a
  Rust-side defence-in-depth filter (BM25 rows are not HQL-filtered).
- **Levelwise batched graph expansion**: one `getConnectionsLevelBatch`
  call per BFS level instead of one `getMemoryLogicalConnections` per
  visited node — O(depth) round-trips.
- **Persistent embedding cache** (`HELIXIR_EMBED_CACHE_PATH`, JSONL,
  model-scoped) with corpus warmup (`HELIXIR_EMBED_CACHE_WARMUP`):
  re-rank phases make zero embedding HTTP calls when warm.
- **LLM-free reasoning chains**: true BFS + cosine-to-query hop selection
  (`ChainGuidance`); seeds widen contextual→full on mature corpora.
- **PPR re-ranking** over the typed ego-network; final rank =
  0.3·cosine + 0.5·ppr + 0.2·freshness. MRR 0.582 → 0.687 on the bench
  golden set.
- **Provenance**: every result's metadata carries origin=seed|graph,
  edge, parent, depth, ppr.
- **`connect_memories(A, B)`** — new MCP tool: bidirectional path
  discovery between two anchors, edge types + cumulative confidence.

## Write path

- **Memory charter, increment 1** (`memory-charter.md`, `core/charter.rs`):
  constitution C1–C5; conflicts surface in `add_memory.needs_clarification`
  (flag-don't-block) with a ready-to-ask question.

## Verification

- `tests/read_path_e2e.rs` (library) + `tests/mcp_read_e2e.rs` (real MCP
  binary over stdio), golden set of 10 queries, run with a dead LLM key.
- Measured on the bench corpus (204 memories): warm search p50 15–30 ms,
  chains 45–90 ms, connect 50–250 ms, MCP transport overhead ~0.2 ms,
  cold start 357 → 80 ms after warmup.

## Known gaps at this snapshot

- Charter blocking semantics, learned rules, memory://rules resource (#34)
- Engine-internal DELETE path still executes (escalated but not blocked) (#34)
- Write path costs 6–22 LLM calls per multi-fact add (#32)
- Relation density: chains depth ~1 on the bench corpus (#33)
- Temporal redesign + bi-temporality (#31, #20)
- graph_depth MCP param ignored; beam width hardcoded (#36)
- Docs overhaul in progress (#30)
