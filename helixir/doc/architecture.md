# Architecture (sysdesign)

> _Reflects code as of `v0.3.1-fix`. Last verified: 2026-05-12._

## 1. System context

```
                     ┌────────────────────────────┐
                     │   IDE / Agent host         │
                     │   (Cursor, Claude Desktop, │
                     │    Codex, any MCP client)  │
                     └─────────────┬──────────────┘
                                   │  MCP over stdio
                                   ▼
   ┌──────────────────────────────────────────────────────────────────┐
   │                       helixir-mcp  (Rust binary)                 │
   │                                                                  │
   │   tools  prompts  resources                                      │
   │   (14)   (2)      (2)                                            │
   └─────────┬────────────────────────────────────────────┬───────────┘
             │ HTTP / HQL                                 │ HTTP / JSON
             ▼                                            ▼
   ┌──────────────────────┐                ┌────────────────────────────┐
   │   HelixDB            │                │   LLM + Embedding APIs     │
   │   graph + vector     │                │   - Cerebras (LLM)         │
   │   :6969              │                │   - OpenAI / OpenRouter    │
   │   ~117 HQL queries   │                │   - Ollama (local)         │
   │   15 nodes / 33 edges│                │                            │
   └──────────────────────┘                └────────────────────────────┘
```

There is also a second binary `helixir-deploy` (used by `install.sh`, `make
setup`, and Ansible) which pushes `schema.hx` and `queries.hx` to HelixDB over
HTTP. It does not participate at runtime.

## 2. Layers

The crate is intentionally layered. Higher layers depend on lower layers, never
the reverse. The layer boundaries are the only place where breaking changes
should require deliberation.

```
┌──────────────────────────────────────────────────────────────────────────┐
│ L5  Process boundary                                                     │
│     src/bin/helixir_mcp.rs        src/bin/helixir_deploy.rs              │
└──────────────────────────────┬───────────────────────────────────────────┘
                               │
┌──────────────────────────────▼───────────────────────────────────────────┐
│ L4  MCP surface                                                          │
│     src/mcp/{server.rs, params.rs, prompts.rs}                           │
│     translates MCP <-> typed Rust calls                                  │
└──────────────────────────────┬───────────────────────────────────────────┘
                               │
┌──────────────────────────────▼───────────────────────────────────────────┐
│ L3  Core facade                                                          │
│     src/core/helixir_client.rs   (HelixirClient — single API door)       │
│     src/core/config.rs           (HelixirConfig + thresholds)            │
│     src/core/events/             (EventBus: register / emit)             │
└──────────────────────────────┬───────────────────────────────────────────┘
                               │
┌──────────────────────────────▼───────────────────────────────────────────┐
│ L2  Tooling pipelines                          src/toolkit/...           │
│                                                                          │
│   tooling_manager/         the orchestrator (add, search, graph, CRUD)   │
│     add_pipeline.rs        2-phase add: personal dedup -> cross-user     │
│     search.rs              search router (dispatch by scope)             │
│     graph.rs               edges, history, user link                     │
│     reasoning.rs           IMPLIES / BECAUSE / CONTRADICTS / SUPPORTS    │
│     crud.rs                update / delete                               │
│                                                                          │
│   mind_toolbox/            domain primitives                             │
│     search/{vector,bm25,hybrid,onto_search,smart_traversal_v2,...}       │
│     entity/                EntityManager                                 │
│     ontology/              OntologyManager (8 concept types)             │
│     reasoning/             ReasoningEngine                               │
│     chunking/              ChunkingManager  (duplicates services/* — #9) │
│     memory/{deletion,remark,...}    soft-delete, supersession, evolution │
│     memory_chain/          chain traversal                               │
│     fast_think/            ephemeral working memory (petgraph)           │
│                                                                          │
│   misc_toolbox/, analytics/                                              │
│                                                                          │
│   NOTE: src/core/services/{chunking,linking,resolution} contains a      │
│   parallel implementation of chunking and link-building alongside        │
│   mind_toolbox/. Consolidation tracked in issue #9.                      │
└──────────────────────────────┬───────────────────────────────────────────┘
                               │
┌──────────────────────────────▼───────────────────────────────────────────┐
│ L1  External adapters                                                    │
│     src/llm/extractor.rs        atomization + entity/relation extraction │
│     src/llm/decision/engine.rs  decide(text, similar_memories)           │
│     src/llm/embeddings.rs       generate / generate_batch / fallback     │
│     src/llm/providers/          cerebras, ollama, fallback (base trait)  │
│     src/db/client.rs            HelixDB HTTP client + retry loop         │
└──────────────────────────────────────────────────────────────────────────┘
```

## 3. Component ownership

Every component has exactly one owner. If you see logic in two places, it is a
bug to file — not a feature to copy.

| Component | File / module | Owns |
|---|---|---|
| MCP server | `src/mcp/server.rs` | Tool dispatch, parameter typing, JSON responses |
| `HelixirClient` | `src/core/helixir_client.rs` | Public facade; nothing else may be a public entry point |
| `HelixirConfig` | `src/core/config.rs` | Configuration shape + env parsing (currently partial, see #10) |
| `EventBus` | `src/core/events/bus.rs` | Side-channel for analytics; nothing on the hot path depends on it |
| `ToolingManager` | `src/toolkit/tooling_manager/` | Pipeline orchestration; the only struct allowed to wire all sub-managers together |
| `ChunkingManager` | `src/toolkit/mind_toolbox/chunking/` | Long-memory chunking + chunk embeddings |
| `EntityManager` | `src/toolkit/mind_toolbox/entity/` | Entity dedup, edges, aliases |
| `OntologyManager` | `src/toolkit/mind_toolbox/ontology/` | Concept hierarchy, classification, mapping |
| `ReasoningEngine` | `src/toolkit/mind_toolbox/reasoning/engine.rs` | IMPLIES / BECAUSE / CONTRADICTS / SUPPORTS edges and traversal |
| `SearchEngine` | `src/toolkit/mind_toolbox/search/mod.rs` | All read paths: vector, BM25, hybrid, smart traversal, onto-search |
| `FastThinkManager` | `src/toolkit/fast_think/` | Ephemeral reasoning sessions on `petgraph` |
| `LlmExtractor` | `src/llm/extractor.rs` | Prompted atomization + structured JSON parsing |
| `LLMDecisionEngine` | `src/llm/decision/engine.rs` | ADD/UPDATE/SUPERSEDE/CONTRADICT/NOOP/LINK_EXISTING/CROSS_CONTRADICT decisions |
| `EmbeddingGenerator` | `src/llm/embeddings.rs` | Vector generation with cache + fallback |
| `HelixClient` | `src/db/client.rs` | HTTP transport to HelixDB + retry |

## 4. Cross-cutting concerns

- **Error type strategy.** Each layer has its own error enum
  (`HelixirError`, `HelixClientError`, `HelixirClientError`, `ToolingError`,
  `SearchError`, `OntologyError`, `FastThinkError`, `DecisionError`,
  `ExtractionError`). The MCP layer flattens them into `McpError` via
  `HelixirMcpServer::convert_error` at `src/mcp/server.rs:50-62`. The
  conversion is lossy: most variants collapse to `internal_error` regardless
  of cause. Whether to unify the error vocabulary is an open design question.

- **Async runtime.** Tokio (`#[tokio::main]`). Most managers are `Send + Sync`
  and held in `Arc<…>`. Two state mutations use synchronous primitives:
  - `OntologyManager` is `parking_lot::RwLock` (sync lock inside async code).
  - `is_initialized` and `is_connected` are `AtomicBool` with `Ordering::Relaxed`.

- **Configuration flow.** Env vars → `HelixirConfig::from_env` → `HelixirClient`
  constructor → passed to every manager. Some `HelixirConfig` fields are not
  read from env (tracked in issue #10) and remain at their struct-literal
  defaults at runtime.

- **Events.** `EventBus` is an async fan-out; handlers run via `tokio::spawn`
  so emit is fire-and-forget. There are currently no registered handlers at
  startup — the bus exists but is unused. If/when analytics are added, this
  is the seam.

- **Caching.** Three caches today:
  1. `moka` future cache inside `EmbeddingGenerator` (LRU 1000, TTL 300s).
  2. `lru::LruCache` inside `SearchEngine` (cache stats exposed via
     `SearchEngine::cache_stats`).
  3. `ReasoningEngine` warm-up cache (`warm_up_cache`, 500 entries).

  Cache sizes are hardcoded at construction (`tooling_manager/mod.rs:65,70`).
  None are configurable from env or `HelixirConfig`.

- **Shared memory across users (deduplicated knowledge graph).** A fact is
  stored exactly once as a `Memory` node, regardless of how many users know it.
  This is a load-bearing invariant for anyone reading API responses.

  Each user that knows the fact is connected to the same node by a
  `User -[HasMemory]-> Memory` edge. The node's `user_count` field tracks how
  many users are linked.

  The flow that creates this in `add_memory`:
  1. New `add_memory` call hits `tooling_manager::add_pipeline`.
  2. If the (content + embedding) closely matches an existing `Memory`, the
     pipeline emits `emit_memory_deduplicated(target_id, user_id)` instead of
     creating a new node (see `add_pipeline.rs:405`).
  3. In a background task, `link_user_to_memory_bg(db, user_id, memory_id)`:
     - `getUser` / `addUser` to make sure the User node exists,
     - `linkUserToMemory` to add the `HasMemory` edge,
     - `getMemoryUsers` to recount, then `updateMemoryUserCount` to persist
       the new `user_count`.

  Consequences for API consumers:
  - `list_memories(user_id=B)` legitimately returns memories whose serialised
    `user_id` field is `A`, with `user_count >= 2`. Those records are linked
    to `B` via `HasMemory`; the `user_id` field is just the original author.
  - `search_memory` honours a `scope` parameter:
    - `personal` — anchor the traversal on the caller's `HasMemory` edges.
    - `collective` / `all` — fan out across all `HasMemory` edges with
      consensus ranking.
  - Tools that do **not** expose `scope` (e.g. `list_memories`,
    `search_by_concept`) implicitly behave like `personal`: they return what
    the user knows, which includes shared knowledge.

  This is not a privacy leak — see the closed-as-`not planned` discussion on
  issue #21.

## 5. Layer boundaries

These boundaries describe how the layers are organized in the source tree.
None are enforced by the compiler today; `test-design.md` notes which of them
have test coverage.

- L4 ↔ L3: every MCP tool maps to exactly one `HelixirClient` method. No
  integration test asserts this contract.
- L3 ↔ L2: `HelixirClient` is the only struct that wires the layer below.
  Unenforced — nothing prevents new MCP tools from importing toolkit modules
  directly.
- L2 ↔ L1: `ToolingManager` owns all `LlmProvider` / `HelixClient` references.
  Sub-managers receive `Arc<…>`s in their constructors and do not pull from
  process-global state at the time of writing.

## 6. Open architectural items

The live architectural backlog is on GitHub:

```bash
gh issue list -R nikita-rulenko/Helixir --label architecture --state open
```

For per-release context, see `<version>/notes.md`.

## 7. Capability surface (what the system provides today)

This section enumerates the user-facing capabilities shipped through the
release history. It is the answer to "what does Helixir actually do?" without
having to grep release notes. Source: `gh release view <tag>` for every tag
plus the root `README.md`.

### 7.1 Memory model

- **Atomic-fact memory.** Every `add_memory` call runs an LLM extractor that
  produces a list of atomic memories from a single user message; the raw
  message itself is stored separately as a `source="raw_input"` Memory when
  the input is long and extraction yielded more than one fact (v0.3.0).
- **8-type ontology.** Memories are classified as one of
  `fact / preference / skill / goal / opinion / experience / achievement /
  action` (v0.2.0). The full hierarchy is the `Thing → {Attribute, Event,
  Entity, Relation, State}` tree in `data-model.md §4`.
- **Decision matrix per write.** The `LLMDecisionEngine` picks one of
  `ADD / UPDATE / SUPERSEDE / CONTRADICT / LINK_EXISTING / CROSS_CONTRADICT
  / NOOP / DELETE` per atomic fact, against the personal-then-collective
  candidate set (v0.2.0 baseline; v0.2.1 wired `LINK_EXISTING` /
  `CROSS_CONTRADICT`; v0.3.1 added coherence guard so `UPDATE` with
  incoherent merged content downgrades to `ADD`).
- **Coherence guard.** `is_coherent_memory` + `split_incoherent_memory`
  detect contradictory clauses across distinct subjects within one candidate
  memory and split at contradiction markers before embedding (v0.3.1).
- **Reasoning edges.** Memory→Memory edges
  `IMPLIES / BECAUSE / CONTRADICTS / SUPPORTS` are inferred during the enrich
  phase of `add_memory` for every operation except `NOOP` / `DELETE`
  (v0.3.1-fix).
- **Audit trail.** Every `UPDATE` / `SUPERSEDE` / `DELETE` writes a
  `HAS_HISTORY` edge to a `HistoryEvent` node.

### 7.2 Retrieval

- **`search_memory`** — vector ANN + BM25 + smart-traversal graph expansion,
  combined by `score = vector_weight * cosine + temporal_weight *
  freshness + graph_weight * proximity`. Real cosine is computed by
  re-embedding the candidate set on the client (v0.3.0). Earlier scoring
  evolved from a hardcoded 0.8 (pre-v0.2.3) → rank-based exp decay
  `0.95 * 0.92^rank` (v0.2.3) → true cosine (v0.3.0).
- **`algo_opt` retrieval profile** (`HELIXIR_RETRIEVAL_PROFILE=algo_opt`,
  branch `local-reasoning`; default `legacy` is bit-for-bit historic
  behaviour). Changes under the flag:
  - Phase 1 fuses dense ANN with **native HelixDB `SearchBM25`** via RRF
    (k=60), query `searchMemoriesByBm25`; temporal cutoff is pushed into
    HQL (`smartVectorSearchWithChunksCutoff`) and re-checked in Rust as
    defence in depth (BM25 rows are not HQL-filtered).
  - Phase 2 graph expansion is **levelwise-batched**: one
    `getConnectionsLevelBatch` HQL call per BFS level
    (`smart_traversal_v2/batch_expansion.rs`) instead of one
    `getMemoryLogicalConnections` call per visited node. Semantics mirror
    the legacy DFS (every unvisited neighbour scored; top-3 per parent
    expand), with a single search-wide visited set.
  - The embedding cache persists to disk (`HELIXIR_EMBED_CACHE_PATH`,
    JSONL, model-scoped, entries never expire) with optional corpus
    warmup at startup (`HELIXIR_EMBED_CACHE_WARMUP=1|blocking`), so
    re-rank phases run with zero embedding HTTP calls once warm.
  - Reasoning chains (`get_chain` with `ChainGuidance`) walk true BFS and
    pick the next hop by **cosine similarity to the query** — the read
    path makes zero LLM calls. Chain seeds widen `contextual → full`
    when the contextual window is empty (mature corpora).
- **Modes.** `recent` (~4 h) · `contextual` (~30 d, default) · `deep`
  (~90 d) · `full` (unbounded). Defined in `src/core/search_modes.rs`.
- **Scopes.** `personal` (caller's `HasMemory` edges) · `collective` /
  `all` (fan out across all `HasMemory` edges with consensus ranking +
  controversy annotation).
- **`search_by_concept`** — ontology-filtered retrieval gated by
  `INSTANCE_OF Concept(type=<one of 8>)`.
- **`search_reasoning_chain`** — BFS over both directions of the four
  reasoning edges; chain modes `forward / causal / both / deep`. Coverage
  was raised from 40 % to ~95 % when traversal grew from 3 to 8 edge
  directions (v0.3.1).
- **`list_memories`** — full-scan tool for exhaustive queries, no scoring
  (v0.3.0).
- **`get_memory_graph`** — return a graph view (nodes + edges) around a
  memory or for a user.
- **`search_incomplete_thoughts`** — locate FastThink sessions that
  auto-committed on timeout (tagged `context_tags=incomplete_thought`).

### 7.3 FastThink (ephemeral working memory)

In-process reasoning scratchpad on `petgraph::stable_graph` — no persistence
until commit. Introduced as the v0.1.1 (`Think_fast`) tag. Tools:
`think_start / think_add / think_recall / think_conclude / think_commit /
think_discard / think_status`. `think_recall` pulls memories from the long-term
store into the live session graph (read-only). On wall-clock or thought-count
timeout the manager runs `commit_partial` and tags the resulting Memory with
`context_tags=incomplete_thought` so it can be recovered later.

Default limits live in `FastThinkLimits::mcp`: 90 s wall clock, 150 thoughts.

### 7.4 Hive Memory (cross-user shared knowledge)

Architectural invariant introduced in v0.2.0 and fixed in v0.2.1:

- One `Memory` node per fact, regardless of how many users know it.
- Each knower is linked to that node by a `User -[HasMemory]-> Memory`
  edge. The node's `user_count` field tracks the linkage count.
- `add_memory` runs a two-phase pipeline:
  - Phase 1 — personal dedup; embedding-similarity match within the
    caller's memories.
  - Phase 2 — collective check (background as of v0.2.2); if the same
    fact already exists for another user, the decision engine emits
    `LINK_EXISTING` and the new user's `HAS_MEMORY` edge points at the
    shared Memory rather than producing a duplicate node.
- Cross-user contradictions are wired through `CROSS_CONTRADICT`, which
  stores the new opinion alongside the existing one and links them with a
  `CONTRADICTS` edge.

### 7.5 Performance & async

- `add_memory` median latency reduced 34.7 s → 12.0 s (v0.2.2) by moving
  the Phase 2 collective LLM decision to `tokio::spawn` and introducing
  `search_for_dedup` (no `user_count` / controversy enrichment).
- Embedding generation is batched on the write path (one HTTP call → N
  vectors). Embedding results are cached by SHA-256(query) via `moka`
  (LRU 1000, TTL 300 s).
- Three caches live in the read path (embeddings, `SearchEngine` LRU,
  `ReasoningEngine` warm-up). All sizes hardcoded at construction.

### 7.6 Reserved capability surface (schema present, no Rust producer)

These are surfaces the schema is ready for but no caller wires today.
They function as the roadmap-by-construction:

| Surface | Schema artifacts | Implication |
|---|---|---|
| Documentation ingestion | `DocPage`, `DocChunk`, `CodeExample`, `ErrorCode` nodes; `PAGE_TO_CHUNK`, `CHUNK_TO_EMBEDDING`, `CHUNK_MENTIONS_CONCEPT`, `CONCEPT_HAS_EXAMPLE`, `ERROR_REFERENCES_CONCEPT` edges | Documents/codebases as first-class memory citizens. |
| Constraint scoping | `Constraint` node; `APPLIES_IN` edge | Per-context rules (work/personal/project). |
| Session tracking | `Session` node; `IN_SESSION`, `CREATED_IN` edges | Conversation-scope reasoning. |
| Internal concept-graph edges | `IS_A`, `CONCEPT_RELATED_TO` edges | Normalized representation of the **fixed** ontology hierarchy and explicit horizontal links between concepts. See note below. |
| Hierarchical entities | `PART_OF` edge | Entity composition (`engine` PART_OF `car`). |

**Note on the ontology surface.** The 8 user-facing ontology types
(`fact / preference / skill / goal / opinion / experience / achievement /
action`) are intentionally **static**. They are not extended at runtime from
user data — that is a deliberate design choice (see
`design-rationale.md §3`). The reserved `IS_A` and `CONCEPT_RELATED_TO`
edges are internal concept-graph machinery: `IS_A` is the normalized form of
the parent link currently denormalized into `Concept.parent_id`, and
`CONCEPT_RELATED_TO` is reserved for explicitly authored horizontal links
between the existing concepts. Neither is intended as a hook for
agent-driven ontology learning.

These are intentional schema surface decisions made in earlier releases
(v0.2.0 for most) and are not dead code in the schema sense — the HQL
queries that materialize them already exist. They are awaiting Rust callers.

### 7.7 Generative-memory agents — `src/agents/` (the Moirai)

Helixir is no longer only an MCP server; it is an **agent** whose MCP surface is
one part. `src/agents/` holds background agents that **compose toolkit
primitives** into behaviour. The layering rule is strict: agent *policy* lives in
`agents/<name>/`; the *capabilities* it drives stay in `toolkit/` (primitives
only). Dependencies flow `agents → toolkit`, never the reverse — the toolkit
knows nothing about agents.

| Agent | Entry | Role |
|---|---|---|
| **Clotho** | `HelixirClient::clotho()` | Tags memories from a controlled category vocabulary (embedding-match, ancestor propagation, charter escalation). Weaves subsets. |
| **Lachesis** | `HelixirClient::lachesis()` | Routes chains within/across subsets and **gates them against apophenia**: a coherence gate (geometric-mean edge weight × reasoning support) and PMI subset-overlap routing (`ln(\|A∩B\|·N / (\|A\|·\|B\|))` — a thick axis gates itself out). Survivors are **hypotheses flagged `requires_verification`** — it proposes, never adjudicates. |
| **Atropos** | — | Curation → insight journal. *(not built)* |

Supporting capabilities (toolkit, this release): the **category subgraph**
(`Category`/`SUBCATEGORY_OF`/`ALIAS_OF`/`TAGGED_AS`), `connect_memories`'
category-bridge axis, **longest-chain reconstruction** (`HelixirClient::
longest_chain`), and **per-edge reasoning weights** now flowing through PPR
ranking + path confidence. In perspective the Moirai run as **N parallel
instances** (memory only grows), supervised inside the daemon (§6 open items).

