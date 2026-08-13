# Architecture (sysdesign)

> _Reflects code as of `v0.14.3`. Last verified: 2026-08-13._

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
   │   178 HQL queries    │                │   - Ollama (local)         │
   │   22 nodes / 30 edges│                │                            │
   └──────────────────────┘                └────────────────────────────┘
```

There is also a second binary `helixir-deploy` (used by `install.sh` and `make
setup`) which pushes `schema.hx` and `queries.hx` to HelixDB over HTTP. It does
not participate at runtime.

Installation is a control plane outside the runtime dependency stack:
`src/installer/` detects machine state, builds a typed installation plan, and
coordinates platform adapters through apply/rollback boundaries. Its client
adapters use native Claude Code/Codex commands and strict Cursor JSON merges;
provider secrets stay outside MCP entries. Embeddings are a closed choice:
recommended local Ollama/Nomic, or an explicitly configured OpenAI-compatible
remote provider. Model adapters install/start Ollama through platform-owned argv
boundaries, wait for its official local API, and retry pulls before verifying
Nomic plus any selected fallback LLM through `/api/tags`. They also pin the
mandatory NLI download to an immutable model revision. Doctor probes the selected
embedding endpoint and visibly repairs a broken remote path by installing
Ollama/Nomic and atomically switching the central config. The provider
factory pins Cerebras requests to `gpt-oss-120b`, while DeepSeek and Ollama
retain independently configured model names. The
backend adapter snapshots the persistent Docker volume before schema changes,
and the manifest records the selected version/models/clients atomically. The
CLI and a future native UI are frontends over this module; they must not own
Docker, model-download, or MCP-client mutation policy themselves. The CLI root is
thin; domain modules live under `src/bin/helixir/`. The same 500-line budget
applies to every maintained Rust source file under `src/`, with
`tests/module_budget.rs` preventing regressions. Large responsibilities are
split into private submodules while their public facades remain stable.

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
│     search/{vector,bm25,hybrid,onto_search,smart_traversal,...}       │
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
| MCP server | `src/mcp/server.rs`, `src/mcp/tools/` | Tool dispatch, parameter typing, JSON responses; memory tools are split by write, read, swarm, and graph responsibility |
| MCP process runtime | `src/mcp/server.rs` | One ingest worker, hot-reload generations, optional gateway bearer authentication |
| `HelixirClient` | `src/core/helixir_client.rs` | Public facade; nothing else may be a public entry point |
| `HelixirConfig` | `src/core/config.rs`, `src/core/config/` | Configuration shape + env parsing (currently partial, see #10) |
| `EventBus` | `src/core/events/bus.rs` | Side-channel for analytics; nothing on the hot path depends on it |
| `ToolingManager` | `src/toolkit/tooling_manager/` | Pipeline orchestration; the only struct allowed to wire all sub-managers together |
| `ChunkingManager` | `src/toolkit/mind_toolbox/chunking/` | Long-memory chunking (storage/reconstruction only — per-chunk vectors rejected in #86) |
| `EntityManager` | `src/toolkit/mind_toolbox/entity/` | Entity dedup, edges, aliases |
| `OntologyManager` | `src/toolkit/mind_toolbox/ontology/` | Concept hierarchy, classification, mapping |
| `ReasoningEngine` | `src/toolkit/mind_toolbox/reasoning/engine.rs` | IMPLIES / BECAUSE / CONTRADICTS / SUPPORTS edges and traversal |
| `SearchEngine` | `src/toolkit/mind_toolbox/search/mod.rs` | All read paths: vector, BM25, hybrid, smart traversal, onto-search |
| `FastThinkManager` | `src/toolkit/fast_think/` | Ephemeral reasoning sessions on `petgraph`; lifecycle and persistence operations are separate private modules |
| `LlmExtractor` | `src/llm/extractor.rs` | Prompted atomization + structured JSON parsing |
| `LLMDecisionEngine` | `src/llm/decision/engine.rs` | ADD/UPDATE/SUPERSEDE/CONTRADICT/NOOP/LINK_EXISTING/CROSS_CONTRADICT decisions |
| `EmbeddingGenerator` | `src/llm/embeddings.rs` | Vector generation with cache + fallback |
| `HelixClient` | `src/db/client.rs` | HTTP transport to HelixDB + retry |
| Installer orchestrator | `src/installer/` | Read-only detection, deterministic install plans, ordered apply/rollback reports, explicit embedding strategies; frontends and platform adapters meet here |
| RBAC policy service | `src/core/rbac.rs`, `src/core/rbac/`, `src/core/rbac_compat.rs`, `src/core/rbac_registry.rs` | Graph-backed policy, administration, memory scoping, compatibility bootstrap, and registry projection |

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
  4. `RbacManager` process cache, keyed by the graph-backed `RbacConfig`
     revision. Every authorization still reads that one config row, so a
     committed grant/revocation invalidates the cached atomic policy snapshot
     immediately without a TTL or second ACL source.

  Cache sizes are hardcoded at construction (`tooling_manager/mod.rs:65,70`).
  None are configurable from env or `HelixirConfig`.

- **Shared memory across users (scoped deduplicated knowledge graph).** Each
  author retains a provenance-preserving `Memory` node. Equivalent records
  share a `content_key`; collective results collapse that fingerprint group
  and derive its holder count.

  The flow that creates this in `add_memory`:
  1. New `add_memory` call hits `tooling_manager::add_pipeline`.
  2. Personal and collective candidate recall is restricted to the resolved
    permanent security domain: reserved `default` for migrated legacy
    fingerprints, otherwise a concrete group or explicit dedup federation.
  3. Exact and NLI-confirmed consensus grouping may unify fingerprints only
     inside that same domain.

  Consequences for API consumers:
  - `Memory.user_id` is provenance, not authorization metadata.
  - `search_memory` honours a `scope` parameter:
    - `personal` — anchor the traversal on the caller's `HasMemory` edges.
    - `collective` / `all` — fan out across all `HasMemory` edges with
      consensus ranking.
  - Tools that do **not** expose `scope` (e.g. `list_memories`,
    `search_by_concept`) implicitly behave like `personal`: they return what
    the user knows, which includes shared knowledge.

  With RBAC enabled, reads are additionally intersected with materialized
  `MEMORY_IN_RBAC_GROUP` edges. Cross-domain rows or dedup candidates are a
  correctness and confidentiality defect.

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
    (`smart_traversal/batch_expansion.rs`) instead of one
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
On SIGHUP, new sessions use the newly built client and limits while sessions
already in progress retain their original runtime generation. The ingest
worker is owned once by the MCP process and reads its current
`ToolingManager` through `ArcSwap`; queue claims are also atomic in HelixDB so
separate stdio/gateway processes cannot process the same `PendingInput`.

### 7.4 Hive Memory (cross-user shared knowledge)

Architectural invariant introduced in v0.2.0 and fixed in v0.2.1:

- One provenance-preserving `Memory` node per author/fact record; equivalent
  records share a security-scoped `content_key` consensus group.
- `HAS_MEMORY` records authorship/stance. Enabled RBAC visibility is independent
  and materialized with `MEMORY_IN_RBAC_GROUP`.
- `add_memory` runs a two-phase pipeline:
  - Phase 1 — personal dedup; embedding-similarity match within the
    caller's memories.
  - Phase 2 — collective check inside the same `rbac_scope`; identical
    author nodes share the scoped fingerprint group.
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
| **Clotho** | `HelixirClient::clotho()` | Tags memories from a controlled, **self-growing** vocabulary — in-memory cosine match; the LLM mints a category on a miss; a **dominance gate** drops noise-floor tags; ancestor propagation; charter escalation. |
| **Lachesis** | `HelixirClient::lachesis()` | Routes chains and **gates them against apophenia**: a coherence gate (geometric-mean edge weight × reasoning support) + PMI subset-overlap (`ln(\|A∩B\|·N / (\|A\|·\|B\|))` — a thick axis gates itself out), **drilling each link to its anchor witnesses**. Survivors are **hypotheses flagged `requires_verification`** — it proposes, never adjudicates. |
| **Atropos** | `HelixirClient::atropos()` | Curates Lachesis threads into ranked, deduplicated `Insight`s with provenance and a lifecycle (`proposed → verified → refuted`). |
| **Orchestrator** | `HelixirClient::orchestrator()` | One `full_pass`: Clotho → Lachesis → Atropos. Choreography (what sequence), kept separate from scheduling (when). |
| **Daemon** | `HelixirClient::daemon()` | Schedules `full_pass` (continuous / on-call). `helixir daemon start/stop/status` runs it detached with a PID file. |

Surface: the **`helixir` CLI** drives + monitors the agents (`categories`, `clotho`, `lachesis`, `atropos`, `pipeline`, `daemon`, `journal`, `insights`) with activity + insight journals, plus **`helixir setup`** to wire the MCP server into agent clients (Claude Code / Desktop / Cursor / Gemini CLI).

Supporting capabilities (toolkit, this release): the **category subgraph**
(`Category`/`SUBCATEGORY_OF`/`ALIAS_OF`/`TAGGED_AS`), `connect_memories`'
category-bridge axis, **longest-chain reconstruction** (`HelixirClient::
longest_chain`), and **per-edge reasoning weights** now flowing through PPR
ranking + path confidence. In perspective the Moirai run as **N parallel
instances** (memory only grows), supervised inside the daemon (§6 open items).
The category bridge is enabled only for the global-admin surface; ordinary RBAC
callers route over the base reasoning graph.

### 7.8 RBAC as a graph-backed policy service

The RBAC layer is a HelixirDB-backed service (`core::rbac::RbacManager`),
not a host-local ACL file. `RbacGroup`, `RbacDedupGroup`, `RbacAssignment`, and
`RbacConfig` provide stable state and audit history; membership, visibility,
and dedup-provenance edges define the security graph. A dedup federation gives
its current groups one fingerprint domain and materializes new memories to
every current member. Detach preserves historical group edges and excludes the
group from future writes. The CLI's `helixir rbac` family is a thin management client
over the same named HQL queries used by MCP and `HelixirClient` authorization.

Bootstrap creates three reserved workspaces. `default` receives pre-RBAC memories
and principals as equal group admins, recreating the historical shared data
plane with unsalted legacy fingerprints. `onboarding` admits newly discovered
principals as workers before an administrator assigns working groups. `moirai`
has no members and stores generated hypotheses in a salted admin-only domain. The
chosen fresh/legacy branch and `pending → migrating → active` phase are stored
in `RbacConfig`, so interruption is resumed idempotently and never rolled back
to disabled enforcement. The service fails closed for unassigned
principals and enforces the role matrix before writes/updates and after reads;
the coarse `HELIXIR_MODE` capability gate remains independent.

Low-level generative and maintenance APIs are exposed through
`HelixirClient::admin_as(actor_id)` only. The raw ToolingManager/client agent
accessors and constructor are crate-private, preventing external Rust callers
and CLI commands such as `categories` from bypassing the same global-admin
decision. FastThink sessions bind their lifecycle to the starting actor;
pending write results and outbox notices retain stricter owner/creator privacy.
Moirai may analyze source memories across all groups under that authority, but
Atropos and Lachesis persist their output only into reserved `moirai`.
`MOIRAI_DERIVED_FROM` preserves witnesses without joining the ordinary reasoning
edge families, so generated hypotheses cannot bridge a non-admin graph walk.

Active or historical membership in either reserved workspace contributes to
the administrative principal registry. `RbacManager::principal_registry` projects User nodes,
active and historical assignments, and matching Agent presence directly from
HelixDB. Removing a membership deactivates assignments without deleting the
User or audit history. The CLI's JSON projection is the contract intended for
the future UI; no UI-owned ACL or registry is permitted.

MCP requests may provide `actor_id` separately from `user_id`. `actor_id` is
the authenticated principal whose grants are evaluated, while `user_id`
remains the memory owner/target. Agents must provide a stable `actor_id`; an
authenticated gateway should populate it explicitly before accepting remote
requests.
