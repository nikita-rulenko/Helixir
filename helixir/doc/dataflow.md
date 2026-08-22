# Dataflow

> _Reflects code as of `v0.17.1`. Last verified: 2026-08-23._

This document walks the two pipelines that matter most:

1. `add_memory` — ingestion (extract → embed → dedup → store → enrich)
2. `search_memory` — retrieval (vector + BM25 + graph + scoring)
3. FastThink commit — the third pipeline (ephemeral → persisted)

Every step is annotated with `file:line` so the diagram and the code remain
welded together.

---

## 1. `add_memory` pipeline

Permanent RBAC makes the facade authorize `(actor_id, owner_id, group_id)`
and resolves a write domain before extraction. A private group uses
`rbac:group:<id>` and a federated group uses `rbac:dedup:<id>`. The reserved
`default` workspace alone keeps the legacy unsalted domain. The domain filters both personal
recall and Phase-2 collective candidates, and is stored as `Memory.rbac_scope`.
After the decision, group visibility is materialized with
`MEMORY_IN_RBAC_GROUP`; federation provenance uses
`MEMORY_IN_RBAC_DEDUP_GROUP`.

When `add_memory` carries an execution `agent_id`, the MCP layer records or
refreshes that concrete presence instance under the already resolved
`actor_id`. The memory owner never determines the agent family. Presence-only
workers use `agent_heartbeat` instead, so lifecycle signalling does not create
synthetic durable memory.

An omitted `group_id` is resolved server-side only when the actor can write to
exactly one reserved workspace in the same policy snapshot used for
authorization. Ambiguous membership fails closed. `default` keeps the legacy
fingerprint and materializes `MEMORY_IN_RBAC_GROUP`; `onboarding` is an ordinary
isolated RBAC scope for new principals.

Federation membership is prospective. Detaching a group leaves historical
memory-to-group edges intact but excludes it from future writes. An in-place
update whose historical visibility differs from the current federation is
forked into a new superseding version; direct mutation of that historical node
is denied.

Buffered completion payloads follow a stricter owner boundary than memory
reads: `get_add_status` is limited to the write owner, creator, or a global
admin, while outbox notices are limited to the owner or a global admin. The
actor-less MCP logging broadcast is disabled under RBAC because no principal
is available to authorize a connection-level notification.

### High-level shape

```
 user_message (str)
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  PHASE 0 — Validation & atomization                                  │
 │    LlmExtractor::extract  (src/llm/extractor.rs)                     │
 │      → ExtractionResult { memories, entities, relations, context }   │
 │      cap: max_facts_per_call (default 15)                            │
 │      fallback: try_parse_extraction → fallback_extraction            │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  PHASE 1 — Batch embed + per-memory dedup                            │
 │    prepare_memories_for_storage (add_pipeline/prepare.rs)            │
 │       splits incoherent memories                                     │
 │       (is_coherent_memory / count_distinct_subjects /                │
 │        split_incoherent_memory)                                      │
 │    EmbeddingGenerator::generate_batch (llm/embeddings/batch.rs)      │
 │       one HTTP call → N vectors                                      │
 │    for each memory i:                                                │
 │       SearchEngine::search(...)  (mind_toolbox/search/dispatch/)     │
 │           mode="contextual" scope="personal" k=5                     │
 │           candidates already restricted to resolved rbac_scope      │
 │       LLMDecisionEngine::decide[_batch] (llm/decision/engine.rs)     │
 │           returns MemoryDecision { op, target_id, confidence, ... }  │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  PHASE 2 — Apply decision                                            │
 │    handle_memory_operation (add_pipeline/decide.rs)                  │
 │                                                                      │
 │      ADD            → store Memory + HAS_EMBEDDING                   │
 │      UPDATE         → mutate target Memory, write HAS_HISTORY        │
 │      SUPERSEDE      → store new + SUPERSEDES edge to old             │
 │      CONTRADICT     → store new + CONTRADICTS edge                   │
 │      LINK_EXISTING  → write MEMORY_RELATION to target                │
 │      CROSS_CONTRADICT → store new + Hive contradiction               │
 │      NOOP           → return early, increment skipped                │
 │      DELETE         → preserve intent, execute as SUPERSEDE          │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  PHASE 3 — Enrichment (per memory)                                   │
 │    add_pipeline/enrich.rs                                            │
 │       ├── EntityManager: MENTIONS / EXTRACTED_ENTITY edges           │
 │       ├── OntologyManager::map_memory_to_concepts                    │
 │       │     → INSTANCE_OF; Clotho later adds TAGGED_AS               │
 │       └── ReasoningEngine: derive typed IMPLIES / BECAUSE /          │
 │                            CONTRADICTS / SUPPORTS relations           │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  PHASE 4 — Extraction-level relations                                │
 │    resolve_and_persist_extraction_relations (add_pipeline/enrich.rs) │
 │       resolves "subject -> predicate -> object" triples to memory    │
 │       ids, persists Memory→Memory or Entity→Entity edges             │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  PHASE 5 — Raw source backup (conditional)                           │
 │    if message.len() > 100 && added > 1:                              │
 │       store_raw_source (add_pipeline/store.rs)                       │
 │         persists the full original message as a Memory tagged        │
 │         memory_type="fact" so the atomized facts can be traced back  │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  PHASE 6 — Background fan-out (fire-and-forget)                      │
 │    cross-user provenance/contradiction (add_pipeline/cross_user.rs)  │
 │    context links                  (add_pipeline/context_link.rs)      │
 │    entity relations               (add_pipeline/entity_links.rs)     │
 │       — all spawned via tokio::spawn; failures only logged           │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 AddMemoryResult { memories_added, memory_ids, updated, deduped,
                   chunks_created, entities_extracted, relations_created,
                   stats, needs_clarification }
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
| `DELETE` | Model proposes removal of the same subject | Charter C1 blocks destruction; store the new fact and execute as `SUPERSEDE` | `SUPERSEDES`, `HAS_HISTORY` |

### Cross-user (Hive) phase

After Phase 2, `apply_cross_user_phase`
(`tooling_manager/add_pipeline/cross_user.rs`) evaluates consensus only inside
the already resolved RBAC security domain. Each author keeps a
provenance-preserving Memory node. Equivalent author nodes share the scoped
`content_key`; collective projection folds that fingerprint family and derives
its holder count instead of leaking or mutating an isolated group's record.
The reserved `default` domain preserves the legacy unsalted key. A private
working group salts by group id, and a dedup federation salts by federation id.

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

The MCP facade resolves `actor_id` first. Every candidate set and graph
expansion is intersected with that actor's materialized
`MEMORY_IN_RBAC_GROUP` visibility before projection. `scope=collective|all`
widens authorship ranking only inside this authorized set; `mode=full` removes
the time bound, not the RBAC bound.

### High-level shape

```
 actor_id, query, user_id, mode, scope, limit,
 temporal_days | time_from/time_to, graph_depth
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
 │    Process cache plus optional private persistent cache; namespace   │
 │    includes provider/endpoint/model revision/dimension/epoch         │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  STEP 3 — SearchEngine::search  (mind_toolbox/search/dispatch/)      │
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
 │    (c) carries HelixDB primary keys and uses                         │
 │    getConnectionsByInternalId per frontier node. This avoids the     │
 │    v2.3.5 label-scan arena retention of the former batch query.      │
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
 │       boosts scoped consensus families with multiple owners         │
 │    fetch_controversy_static                                          │
 │       annotates collective results with contradiction count          │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌──────────────────────────────────────────────────────────────────────┐
 │  STEP 6 — Curate and project                                         │
 │    demote superseded rows; collapse raw/atom families; preserve      │
 │    provenance; keep `limit` in-window rows plus a separate bounded   │
 │    flashback allowance                                               │
 └──────────────────────────────────────────────────────────────────────┘
       │
       ▼
 Vec<SearchResult> { id, content, score, metadata, created_at }
```

### Event-time windows and flashback projection

`time_from`/`time_to` create an inclusive event-time `TimeWindow`; either side
may be open. RFC3339 values are used directly and a bare `YYYY-MM-DD` expands
to the start of day for the lower bound or end of day for the upper bound. An
explicit window overrides `temporal_days`; malformed bounds and an inverted
window fail at the MCP boundary.

The contract deliberately separates attention from reachability:

1. vector/BM25 seeds are hard-filtered to the window;
2. event time is `valid_from` when present and otherwise `created_at`;
3. RBAC visibility is intersected before and after expansion exactly as for an
   unbounded query;
4. graph expansion may cross the temporal boundary;
5. every out-of-window expansion row is labelled
   `metadata.flashback=true` with its real `metadata.event_date`;
6. final projection keeps up to `limit` in-window rows and then appends at most
   `retrieval.flashback_max` flashbacks (default 3), so linked history cannot
   crowd out the requested period.

This is why a search for “what happened in June” may honestly return June
events plus a dated May causal predecessor. The caller must describe the latter
as related older context, not rewrite the timeline. Reasoning-chain tools walk
their authorized graph by definition and are not converted into period-event
queries.

### Result-family and history projection

Two other projections keep recall useful without erasing graph history:

- raw long-form source memories and their extracted atoms form a `PART_OF`
  family. Only the best-ranked representative appears in one result window;
  folded ids remain in `metadata.collapsed` and are still addressable;
- a row with an incoming `SUPERSEDES` edge remains reachable but receives the
  `retrieval.superseded_penalty` (default 0.6) and is labelled
  `superseded=true` plus `superseded_by`. Consumers must follow the successor
  for current truth.

Collective projection then folds equivalent scoped fingerprints and annotates
holder/controversy information only across owners already visible to the actor.
None of these projections widens RBAC access or deletes physical history.

### Specialized search variants

All re-use the same `SearchEngine` instance:

- `search_by_concept` — adds an ontology filter (`INSTANCE_OF Concept(type=…)`)
  before scoring. Lives at `tooling_manager/search/manager.rs`.
- `search_reasoning_chain` — seeds from `search`, then traverses
  IMPLIES/BECAUSE/CONTRADICTS/SUPPORTS up to `max_depth` (default 5). Lives
  at `tooling_manager/reasoning.rs`.
- `connect_memories` — resolves two semantic anchors, then returns one
  authorized typed path (plus confidence) between them. It is the bridge tool
  for “how are A and B related?”, not an exhaustive graph export.
- `get_memory_graph` — projects a bounded authorized neighborhood around a
  memory, or a bounded owner view when no memory id is supplied.
- `list_memories` — bounded newest-first audit view without semantic ranking;
  it is for inspection, not the default recall path.
- `search_for_dedup` — internal variant used by Phase 1 of add_memory, top-k
  small (5), bypasses the moka cache to avoid stale dedup decisions.
  `mind_toolbox/search/dispatch/projection.rs`.
- `search_by_tag` — exact match on `Memory.context_tags`. Used by
  `search_incomplete_thoughts`.

---

## 3. FastThink commit pipeline

FastThink keeps a `petgraph::stable_graph::StableDiGraph<Thought, Relation>`
in-process. Only `think_commit` mutates HelixDB.

Each session pins the client and limits from the runtime generation in which
it started. Permanent RBAC also pins the authenticated `actor_id`, and
every lifecycle operation validates that binding before exposing or mutating
the scratchpad. A SIGHUP publishes a new generation for future sessions
without changing an active reasoning graph.

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
 │  FastThinkManager::commit (fast_think/manager/persistence.rs)│
 │     1. Collects explicit conclusions only.                   │
 │     2. Short conclusions enter add_memory as prepared atoms; │
 │        long text takes the normal extractor path.            │
 │     3. Recalled persistent evidence gets SUPPORTS edges to   │
 │        committed conclusions; scratch thoughts are not saved.│
 │     4. Entity discovery skipped by the fast path is deferred │
 │        after the commit acknowledgement.                     │
 └──────────────────────────────────────────────────────────────┘

 think_discard ─► drops the in-memory graph; nothing touches HelixDB.
```

Timeout behavior: each session has a configured wall-clock and thought limit
(90 s / 150 thoughts by default). Permanent RBAC returns an error and retains
the bound scratchpad for explicit discard, because the lifecycle call has no
concrete owner/group with which to authorize a partial persistent write.

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

## 5. What the diagrams do not show

- **Hot/cold path separation.** Embedding cache hit vs miss, HelixDB retry
  loop iterations, and decision-engine LLM call cost are not annotated.
- **Concurrency boundaries.** Deferred entity linking and selected enrichment
  work run after the caller's acknowledgement; failures are logged and do not
  rewrite the acknowledged decision.
- **Backpressure.** The optional ingest buffer has one process-owned worker and
  an atomic HelixDB claim contract across processes. Direct writes still pay
  provider concurrency immediately; provider-specific rate limiting remains an
  external deployment concern.
- **Browser projection cadence.** The category atlas is a separate read-only
  control-plane cache refreshed every five minutes. It never participates in
  MCP retrieval, dedup or authorization decisions.
