# Dataflow

> _Reflects code as of `v0.3.1-fix`. Last verified: 2026-05-12._

This document walks the two pipelines that matter most:

1. `add_memory` — ingestion (extract → embed → dedup → store → enrich)
2. `search_memory` — retrieval (vector + BM25 + graph + scoring)
3. FastThink commit — the third pipeline (ephemeral → persisted)

Every step is annotated with `file:line` so the diagram and the code remain
welded together.

---

## 1. `add_memory` pipeline

### High-level shape

```
 user_message (str)
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  PHASE 0 — Validation & atomization                                  │
 │    LlmExtractor::extract  (src/llm/extractor.rs:101)                 │
 │      → ExtractionResult { memories, entities, relations, context }   │
 │      cap: max_facts_per_call (default 15)                            │
 │      fallback: try_parse_extraction → fallback_extraction            │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  PHASE 1 — Batch embed + per-memory dedup                            │
 │    prepare_memories_for_storage (add_pipeline.rs:190)                │
 │       splits incoherent memories                                     │
 │       (is_coherent_memory / count_distinct_subjects /                │
 │        split_incoherent_memory)                                      │
 │    EmbeddingGenerator::generate_batch (llm/embeddings.rs:?)          │
 │       one HTTP call → N vectors                                      │
 │    for each memory i:                                                │
 │       SearchEngine::search(...)  (mind_toolbox/search/mod.rs:144)    │
 │           mode="contextual" scope="personal" k=5                     │
 │       LLMDecisionEngine::decide  (llm/decision/engine.rs:100)        │
 │           returns MemoryDecision { op, target_id, confidence, ... }  │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  PHASE 2 — Apply decision                                            │
 │    handle_memory_operation (add_pipeline.rs:329, 13 args; see #9)    │
 │                                                                      │
 │      ADD            → store Memory + HAS_EMBEDDING                   │
 │      UPDATE         → mutate target Memory, write HAS_HISTORY        │
 │      SUPERSEDE      → store new + SUPERSEDES edge to old             │
 │      CONTRADICT     → store new + CONTRADICTS edge                   │
 │      LINK_EXISTING  → write MEMORY_RELATION to target                │
 │      CROSS_CONTRADICT → store new + Hive contradiction               │
 │      NOOP           → return early, increment skipped                │
 │      DELETE         → soft-delete target                             │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  PHASE 3 — Enrichment (per memory)                                   │
 │    enrich_memory_relations (add_pipeline.rs:562)                     │
 │       ├── EntityManager: MENTIONS / EXTRACTED_ENTITY edges           │
 │       ├── OntologyManager::map_memory_to_concepts                    │
 │       │     → INSTANCE_OF / BELONGS_TO_CATEGORY                      │
 │       └── ReasoningEngine: derive IMPLIES / BECAUSE / CONTRADICTS    │
 │                            / SUPPORTS edges                          │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  PHASE 4 — Extraction-level relations                                │
 │    resolve_and_persist_extraction_relations (add_pipeline.rs:690)    │
 │       resolves "subject -> predicate -> object" triples to memory    │
 │       ids, persists Memory→Memory or Entity→Entity edges             │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  PHASE 5 — Raw source backup (conditional)                           │
 │    if message.len() > 100 && added > 1:                              │
 │       store_raw_source (add_pipeline.rs:932)                         │
 │         persists the full original message as a Memory tagged        │
 │         memory_type="fact" so the atomized facts can be traced back  │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  PHASE 6 — Background fan-out (fire-and-forget)                      │
 │    link_user_to_memory_bg   (add_pipeline.rs:1171)                   │
 │    add_contradiction_bg     (add_pipeline.rs:1211)                   │
 │    link_memory_to_extracted_context (add_pipeline.rs:1081)           │
 │    persist_entity_relation (add_pipeline.rs:1013)                    │
 │       — all spawned via tokio::spawn; failures only logged           │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 AddMemoryResult { memories_added, memory_ids, chunks_created,
                   entities_extracted, relations_created, stats }
```

### Decision matrix

`LLMDecisionEngine::decide` returns one of these operations (see
`src/llm/decision/models.rs:10`):

| Operation | Trigger | Effect | Edges written |
|---|---|---|---|
| `ADD` | No similar memory above `similarity_threshold` (0.70) | Store new Memory | `HAS_EMBEDDING`, `HAS_MEMORY` |
| `UPDATE` | Target memory subsumes new content | Mutate target content, regen embedding | `HAS_HISTORY` |
| `SUPERSEDE` | New memory contradicts older one and is preferred | Store new, mark old as superseded | `SUPERSEDES`, `HAS_HISTORY` |
| `CONTRADICT` | New memory contradicts existing of same user | Store new alongside old, link | `CONTRADICTS` |
| `LINK_EXISTING` | New memory is related, not duplicate | No new Memory; relation only | `MEMORY_RELATION` |
| `NOOP` | Exact duplicate (score ≥ `exact_duplicate_score`, 0.98) | Skip | — |
| `CROSS_CONTRADICT` | Hive contradiction with another user's memory | Store new + Hive contradiction | `CONTRADICTS` |
| `DELETE` | Explicit removal directive | Soft-delete via `is_deleted` flag | `HAS_HISTORY` |

### Cross-user (Hive) phase

After Phase 2, `apply_cross_user_phase` (`add_pipeline.rs:477`) re-runs the
search with `scope="collective"`. If the same fact already exists for another
user, `user_count` on that Memory is incremented and the new user's
`HAS_MEMORY` edge points to the shared Memory instead of creating a duplicate.

### Failure modes

- **LLM extraction returns invalid JSON** → `try_parse_extraction` falls back
  to `fallback_extraction` (a single Memory of `memory_type="fact"` with the
  raw text). Atomization is lost but persistence continues.
- **Embedding API timeout** → retries via `EmbeddingGenerator` fallback URL;
  if both fail, the whole `add_memory` call returns `EmbeddingError`.
- **HelixDB query failure** → `HelixClient::execute_query` retries 3 times
  with exponential backoff (100 ms → 200 ms → 400 ms, capped 10 s). Only
  "not found" / "no value" errors bypass retries.
- **Background tasks failing** → logged at `warn!`, never surfaced to caller.

---

## 2. `search_memory` pipeline

### High-level shape

```
 query (str), user_id, mode, scope, limit, temporal_days, graph_depth
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  STEP 1 — Mode resolution                                            │
 │    src/core/search_modes.rs                                          │
 │      recent      → ~4h window, fast                                  │
 │      contextual  → ~30d window, balanced  (default in code)          │
 │      deep        → ~90d window                                       │
 │      full        → unbounded                                         │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  STEP 2 — Query embedding                                            │
 │    EmbeddingGenerator::generate  (single)                            │
 │    Cached by SHA-256(query) via moka (TTL 300 s)                     │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  STEP 3 — SearchEngine::search  (mind_toolbox/search/mod.rs:144)     │
 │                                                                      │
 │    Three sub-searches run with shared SearchEngineConfig             │
 │    (thresholds from HelixirConfig.search_thresholds):                │
 │                                                                      │
 │    a) Vector search (k * 3) — HelixDB ANN over MemoryEmbedding       │
 │       src/toolkit/mind_toolbox/search/vector.rs                      │
 │                                                                      │
 │    b) BM25 over candidate set (bm25_k1=1.5, bm25_b=0.75)             │
 │       src/toolkit/mind_toolbox/search/bm25.rs                        │
 │                                                                      │
 │    c) Smart-traversal v2: graph expansion from seed memories         │
 │       src/toolkit/mind_toolbox/search/smart_traversal/            │
 │       — walks all 8 reasoning-related edges + 33 edge directions     │
 │                                                                      │
 │    Under HELIXIR_RETRIEVAL_PROFILE=algo_opt (see architecture.md     │
 │    §7.2): (b) is HelixDB-native SearchBM25 fused via RRF k=60, and   │
 │    (c) runs levelwise-batched — one getConnectionsLevelBatch call    │
 │    per BFS level (batch_expansion.rs) instead of one call per node.  │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  STEP 4 — Combined scoring                                           │
 │    src/toolkit/mind_toolbox/search/smart_traversal/scoring.rs     │
 │                                                                      │
 │       score = vector_weight    * cosine_similarity                   │
 │             + temporal_weight  * temporal_freshness                  │
 │             + graph_weight     * graph_proximity                     │
 │                                                                      │
 │       weights from HelixirConfig.search_thresholds (defaults:        │
 │       0.7 / 0.3 / 0.5 — note these don't sum to 1; relative only)    │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  STEP 5 — Hive scope adjustment (if scope != "personal")             │
 │    fetch_memory_user_count_static                                    │
 │       boosts memories shared across users                            │
 │    fetch_controversy_static                                          │
 │       annotates collective results with contradiction count          │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  STEP 6 — Apply min_combined_score (0.3) and limit                   │
 │    SearchResult vec; sorted by score desc                            │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 Vec<SearchResult> { id, content, score, metadata, created_at }
```

### Specialized search variants

All re-use the same `SearchEngine` instance:

- `search_by_concept` — adds an ontology filter (`INSTANCE_OF Concept(type=…)`)
  before scoring. Lives at `tooling_manager/search.rs:150`.
- `search_reasoning_chain` — seeds from `search`, then traverses
  IMPLIES/BECAUSE/CONTRADICTS/SUPPORTS up to `max_depth` (default 5). Lives
  at `tooling_manager/reasoning.rs`.
- `search_for_dedup` — internal variant used by Phase 1 of add_memory, top-k
  small (5), bypasses the moka cache to avoid stale dedup decisions.
  `mind_toolbox/search/mod.rs:331`.
- `search_by_tag` — exact match on `Memory.context_tags`. Used by
  `search_incomplete_thoughts`.

---

## 3. FastThink commit pipeline

FastThink keeps a `petgraph::stable_graph::StableDiGraph<Thought, Relation>`
in-process. Only `think_commit` mutates HelixDB.

```
 think_start ─► creates session in memory only
 think_add   ─► adds Thought node (Reasoning / Hypothesis /
                Observation / Question) — graph stays in RAM
 think_recall─► reads from HelixDB into the graph (read-only)
 think_conclude ─► marks a thought as the conclusion + supporting indices
 think_commit
        │
        ▼
 ┌──────────────────────────────────────────────────────────────┐
 │  FastThinkManager::commit (toolkit/fast_think/manager.rs)    │
 │     1. Builds a single Memory from the conclusion text.      │
 │     2. Calls HelixirClient::add (full add_memory pipeline)   │
 │        with metadata.context = "fast_think_commit".          │
 │     3. For each supporting Thought:                          │
 │        - Materializes it as a Memory.                        │
 │        - Adds SUPPORTS edge → conclusion Memory.             │
 │     4. Emits an `incomplete_thought` tag if the session      │
 │        timed out (commit_partial path).                      │
 └──────────────────────────────────────────────────────────────┘

 think_discard ─► drops the in-memory graph; nothing touches HelixDB.
```

Timeout behavior: each session has a wall-clock limit (`FastThinkLimits::mcp`
defaults to 90 s, 150 thoughts). On timeout during `think_add`, the manager
auto-runs `commit_partial`, tagging the resulting Memory with
`context_tags: incomplete_thought`. The MCP tool `search_incomplete_thoughts`
recovers them.

---

## 4. EventBus (side-channel)

`EventBus` (`src/core/events/bus.rs`) is wired into `ToolingManager` but has
no registered subscribers at startup. Emit-points exist (e.g. `tooling.emitters`
in `add_pipeline`), they enqueue events into the bus, and the events are
dropped because no handler is registered.

If/when telemetry is added, it hooks here. Until then, emitted events have no
observable effect:

```
┌────────────────────┐       ┌──────────────┐       ┌────────────────┐
│ tooling pipelines  │ emit  │   EventBus   │ spawn │  handler(s)    │
│  add / update /    │──────►│  (async)     │──────►│   (none today) │
│  search / delete   │       │   register() │       │                │
└────────────────────┘       └──────────────┘       └────────────────┘
```

## 5. What the diagrams do NOT show (yet)

- **Hot/cold path separation.** Embedding cache hit vs miss, HelixDB retry
  loop iterations, and decision-engine LLM call cost are not annotated.
- **Concurrency boundaries.** Several `tokio::spawn` calls inside
  `add_pipeline.rs` run as fire-and-forget tasks; failures are visible only
  in logs.
- **Backpressure.** None today; concurrent `add_memory` calls all hit the
  embedding API + LLM without queueing or rate-limiting.

Candidates for the next iteration of this document. Tracked alongside #9.
