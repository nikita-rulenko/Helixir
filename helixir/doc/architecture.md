# Architecture (sysdesign)

> _Reflects code as of `v0.17.2`. Last verified: 2026-08-23._

## 1. System context

```
                     ┌────────────────────────────┐
                     │   IDE / Agent host         │
                     │   (Cursor, Claude Desktop, │
                     │    Codex, any MCP client)  │
                     │   helixir-client bootstrap │
                     └─────────────┬──────────────┘
                                   │  streamable HTTP (preferred)
                                   │  or stdio fallback
                                   ▼
   ┌──────────────────────────────────────────────────────────────────┐
   │        helixir gateway (one per host) / helixir-mcp fallback    │
   │                                                                  │
   │   tools  prompts  resources                                      │
   │   (23)   (2)      (3)                                            │
   └─────────┬────────────────────────────────────────────┬───────────┘
             │ HTTP / HQL                                 │ HTTP / JSON
             ▼                                            ▼
   ┌──────────────────────┐                ┌────────────────────────────┐
   │   HelixDB            │                │   LLM + Embedding APIs     │
   │   graph + vector     │                │   - Cerebras (LLM)         │
   │   :6969              │                │   - OpenAI / OpenRouter    │
   │   190 HQL queries    │                │   - Ollama (local)         │
   │   22 nodes / 30 edges│                │                            │
   └──────────────────────┘                └────────────────────────────┘
```

There is also a second binary `helixir-deploy` (used by `install.sh` and `make
setup`) which pushes `schema.hx` and `queries.hx` to HelixDB over HTTP. It does
not participate at runtime.

`helixir-client/` is a separate Rust crate, release archive, Homebrew formula,
and Debian package for an agent-only host. It depends on neither the `helixir`
crate nor ONNX/model/database code, and its package owns no server executable.
It speaks streamable HTTP to an existing gateway, requests only bounded
onboarding admission, installs verified MCP registrations plus canonical
instructions, and persists only a non-secret local profile. Keeping it in the
same repository preserves one protocol/version/CI contract without turning the
full server crate into a client dependency.

Installation is a control plane outside the runtime dependency stack:
`src/installer/` exposes one `InstallerService` that detects machine state,
builds a typed installation plan, applies it and verifies the result. Both the
CLI and browser supervisor call this same service. The concrete
`NativeInstallExecutor` and its Docker, model, client-registration, config,
RBAC and doctor adapters live in the library; frontends own only prompts,
consent and rendering. Its client
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
CLI and browser control plane are frontends over this module; they
must not own Docker, model-download, or MCP-client mutation policy themselves.
The HTML5/CSS3/Tailwind SPA and its Axum backend are packaged only in the
`helixir-control-plane` image, beside but separate from the HelixDB container.
The image is non-root and read-only, binds through a host-loopback publication,
and receives neither `docker.sock` nor host home directories. Dashboard reads
go directly to HelixDB under global-admin RBAC. Host discovery and deterministic
plan construction cross a narrow bearer-authenticated native supervisor; the
container receives only its read-only token file. Browser authorization uses a
second, persistent 64-hex-character token under `~/.helixir/run/`; it survives
container restarts and is never reused as host-supervisor authority. A configured
container fails closed when either secret is absent or malformed. The versioned
API additionally rejects mismatched browser origins and cross-site fetch metadata,
bounds request bodies to 1 MiB, emits typed secret-safe problems for authentication
and routing failures, and marks every API response `no-store`; it intentionally
publishes no CORS capability. If the host
bridge is absent or unreachable, host operations fail closed instead of inspecting
the container's namespace and pretending it is the host. Long-running installation
applies are owned by that supervisor as durable operations under
`~/.helixir/run/operations/`: each typed event has a stable cursor, files are
atomically replaced with private permissions, and an in-flight record becomes
explicitly `interrupted` and resumable after process restart. The web backend
proxies authenticated SSE from a cursor and never persists `InstallOptions` or
provider secrets; resume first rebuilds the plan and requires the original
fingerprint. Post-installation administration uses the same bridge: `/settings`
returns a curated effective configuration with write-only secret replacement,
while `/backups` accepts only managed archive identifiers. Settings are
allowlisted, validated as a complete configuration, backed up and atomically
replaced. A restore verifies the archive, creates a fresh safety snapshot,
restarts HelixDB, proves the current schema contract and rolls back on
incompatibility. Browser requests never carry host filesystem paths. The CLI root is thin;
domain modules live under
`src/bin/helixir/`. The same 500-line budget
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
│     src/core/helixir_client/     (HelixirClient — single API door)       │
│     src/core/config.rs           (HelixirConfig + thresholds)            │
│     src/core/events/             (EventBus: register / emit)             │
└──────────────────────────────┬───────────────────────────────────────────┘
                               │
┌──────────────────────────────▼───────────────────────────────────────────┐
│ L2  Tooling pipelines                          src/toolkit/...           │
│                                                                          │
│   tooling_manager/         the orchestrator (add, search, graph, CRUD)   │
│     add_pipeline/          scoped recall -> decision -> store/enrich     │
│     search/                search router, RBAC projection, scoring        │
│     graph.rs               edges, history, user link                     │
│     reasoning.rs           IMPLIES / BECAUSE / CONTRADICTS / SUPPORTS    │
│     crud.rs                update + operator-only repair purge           │
│                                                                          │
│   mind_toolbox/            domain primitives                             │
│     search/{vector,bm25,hybrid,onto_search,smart_traversal,...}       │
│     entity/                EntityManager                                 │
│     ontology/              OntologyManager (8 concept types)             │
│     reasoning/             ReasoningEngine                               │
│     chunking/              ChunkingManager (storage/reconstruction)       │
│     memory/{crud,remark,...}        supersession/evolution primitives    │
│     memory_chain/          chain traversal                               │
│     fast_think/            ephemeral working memory (petgraph)           │
│                                                                          │
│   misc_toolbox/, analytics/                                              │
└──────────────────────────────┬───────────────────────────────────────────┘
                               │
┌──────────────────────────────▼───────────────────────────────────────────┐
│ L1  External adapters                                                    │
│     src/llm/extractor.rs        atomization + entity/relation extraction │
│     src/llm/decision/engine.rs  decide(text, similar_memories)           │
│     src/llm/embeddings/         generate / batch / cache / wire fallback │
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
| Thin remote client | `../helixir-client/` | Gateway handshake, bounded self-enrollment, backup-verified Codex/Claude/Cursor registration, canonical skill/AGENTS installation and client-scoped doctor; owns no server services |
| `HelixirClient` | `src/core/helixir_client/` | Public facade; nothing else may be a public entry point |
| `HelixirConfig` | `src/core/config.rs`, `src/core/config/` | Layered defaults/TOML/environment configuration and runtime validation |
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
| `EmbeddingGenerator` | `src/llm/embeddings/` | Vector generation, provider/fallback wire paths, versioned in-memory/persistent cache and diagnostics |
| `HelixClient` | `src/db/client.rs` | HTTP transport to HelixDB + retry |
| Installer orchestrator | `src/installer/service.rs`, `src/installer/executor/` | One detect/prepare/apply/verify service, deterministic plans, concrete native mutation adapters, ordered rollback reports and explicit embedding strategies; frontends provide presentation and consent only |
| Web control plane | `src/control_plane/`, `web/` | Global-admin-only versioned HTTP API and compiled browser shell. Projects overview counters/mode, advertised MCP client handoff, graph-derived onboarding placement, RBAC principals/groups/dedup mutations and permission checks, swarm presence/pruning, a bounded group/identity-aware memory graph, admin-only Moirai witness provenance, Hygieia telemetry, the machine-checked physical schema ledger/census, redacted settings and the managed backup vault; never owns host mutation policy |
| Control-plane image | `Dockerfile` (`control-plane` / artifact-only `release-control-plane` targets), `docker-compose.yml` | Immutable frontend/backend packaging and the restricted runtime boundary; releases reuse native ABI-gated binaries and a shared frontend artifact rather than recompiling Rust; no host filesystem or Docker authority |
| Native host supervisor | `src/installer/supervisor.rs`, `src/installer/operations.rs`, `src/installer/{settings,backups}.rs`, `src/control_plane/supervisor.rs` | Authenticated bridge for host discovery, plan construction, durable cursor-based install operations, redacted atomic settings, guarded managed-volume backup/restore, a bounded Hygieia health/journal projection, and an allowlisted set of typed lifecycle operations; exposes no general shell or filesystem endpoint |
| RBAC policy service | `src/core/rbac.rs`, `src/core/rbac/`, `src/core/rbac_compat.rs`, `src/core/rbac_registry.rs` | Graph-backed policy, administration, memory scoping, compatibility bootstrap, and registry projection |

## 4. Cross-cutting concerns

- **Error type strategy.** Each layer has its own error enum
  (`HelixirError`, `HelixClientError`, `HelixirClientError`, `ToolingError`,
  `SearchError`, `OntologyError`, `FastThinkError`, `DecisionError`,
  `ExtractionError`). The MCP layer flattens them into `McpError` via
  `HelixirMcpServer::convert_error` in `src/mcp/server.rs`. The
  conversion is lossy: most variants collapse to `internal_error` regardless
  of cause. Whether to unify the error vocabulary is an open design question.

- **Async runtime.** Tokio (`#[tokio::main]`). Most managers are `Send + Sync`
  and held in `Arc<…>`. Two state mutations use synchronous primitives:
  - `OntologyManager` is `parking_lot::RwLock` (sync lock inside async code).
  - `is_initialized` and `is_connected` are `AtomicBool` with `Ordering::Relaxed`.

- **Configuration flow.** Built-in defaults → the first available central
  file (`$HELIXIR_CONFIG`, `~/.helixir/helixir.toml`, then `./helixir.toml`) →
  `HELIX_*`/`HELIXIR_*` environment overrides → `HelixirClient` and managers.
  CLI `config` and the global-admin Stewardship room mutate only an allowlisted
  subset of the central file, validate cross-field invariants, replace it
  atomically, redact secrets, and use the shared reload coordinator.

- **Events.** `EventBus` is an async fan-out; handlers run via `tokio::spawn`
  so emit is fire-and-forget. There are currently no registered handlers at
  startup — the bus exists but is unused. If/when analytics are added, this
  is the seam.

- **Caching.** Five bounded caches today:
  1. `EmbeddingGenerator` keeps a process-local LRU+TTL cache. Optional
     persistence adds a locked, byte-bounded JSONL store whose namespace covers
     format, provider, endpoint, model revision, vector dimension and explicit
     epoch. Only a SHA-256 text key is persisted; raw memory text is not.
  2. `lru::LruCache` inside `SearchEngine` (cache stats exposed via
     `SearchEngine::cache_stats`).
  3. `ReasoningEngine` warm-up cache (`warm_up_cache`, 500 entries).
  4. `RbacManager` process cache, keyed by the graph-backed `RbacConfig`
     revision. Every authorization still reads that one config row, so a
     committed grant/revocation invalidates the cached atomic policy snapshot
     immediately without a TTL or second ACL source.
  5. The browser control plane caches bounded projection snapshots and refreshes
     them asynchronously; identity/group filters request another bounded slice
     rather than materializing the entire graph in the browser.

  Embedding entry count/TTL come from `llm_runtime`, and persistent-cache path,
  byte ceiling, revision, dimension, epoch and warm-up mode have explicit
  environment controls. RBAC invalidation remains graph-revision driven rather
  than time driven.

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

  Under permanent RBAC, reads are additionally intersected with materialized
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
  / NOOP / DELETE` per atomic fact, against owner-then-cross-owner candidates
  inside one resolved RBAC security domain (v0.2.0 baseline; v0.2.1 wired `LINK_EXISTING` /
  `CROSS_CONTRADICT`; v0.3.1 added coherence guard so `UPDATE` with
  incoherent merged content downgrades to `ADD`).
- **Coherence guard.** `is_coherent_memory` + `split_incoherent_memory`
  detect contradictory clauses across distinct subjects within one candidate
  memory and split at contradiction markers before embedding (v0.3.1).
- **Reasoning edges.** Seven typed Memory→Memory semantics are available.
  `IMPLIES / BECAUSE / CONTRADICTS` use dedicated physical edges;
  `SUPPORTS / RELATES_TO / PART_OF / IS_A` use the generic
  `MEMORY_RELATION` shape. The four logical/causal relations are inferred
  during the enrich phase of `add_memory` for every operation except
  `NOOP` / `DELETE` (v0.3.1-fix).
- **Audit trail.** Every `UPDATE` / `SUPERSEDE` / `DELETE` writes a
  `HAS_HISTORY` edge to a `HistoryEvent` node.
- **Confirm-or-promise ingestion.** `add_memory` returns a completed
  `memory_ids`/`updated`/`deduped` result or an accepted `pending_id`. Atomic
  HelixDB claims prevent multiple stdio/gateway workers from processing the
  same pending write; status and outbox reads retain owner/admin boundaries.

### 7.2 Retrieval

- **`search_memory`** — vector ANN + BM25 + smart-traversal graph expansion,
  combined by `score = vector_weight * cosine + temporal_weight *
  freshness + graph_weight * proximity`. Real cosine is computed by
  re-embedding the candidate set on the client (v0.3.0). Earlier scoring
  evolved from a hardcoded 0.8 (pre-v0.2.3) → rank-based exp decay
  `0.95 * 0.92^rank` (v0.2.3) → true cosine (v0.3.0).
- **`algo_opt` retrieval profile** (`HELIXIR_RETRIEVAL_PROFILE=algo_opt`) is
  the default since v0.4.0. Explicit `legacy` remains only as a v0.3.x
  compatibility/debug profile. The optimized bundle provides:
  - Phase 1 fuses dense ANN with **native HelixDB `SearchBM25`** via RRF
    (k=60), query `searchMemoriesByBm25`; temporal cutoff is pushed into
    HQL (`smartVectorSearchWithChunksCutoff`) and re-checked in Rust as
    defence in depth (BM25 rows are not HQL-filtered).
  - Phase 2 graph expansion is **levelwise and primary-key anchored**.
    `smart_traversal/batch_expansion.rs` calls
    `getConnectionsByInternalId` for each bounded frontier node instead of
    scanning the Memory label with an `IS_IN` batch predicate. This is more
    local round trips than the original batch proposal, but avoids HelixDB
    v2.3.5 request-arena growth (#89). `getConnectionsLevelBatch` remains the
    bounded primitive for path/longest-chain consumers.
  - The embedding cache optionally persists to private JSONL
    (`HELIXIR_EMBED_CACHE_PATH`) with corpus warmup at startup
    (`HELIXIR_EMBED_CACHE_WARMUP=1|blocking`). Durable keys contain a
    versioned namespace over provider, normalized endpoint, model, optional
    artifact revision, expected dimension, and explicit cache epoch. Reachable
    Ollama aliases are resolved to their `/api/tags` digest before the durable
    cache opens; opaque remote aliases use the operator-controlled epoch. Text is
    represented only by SHA-256. Startup scans the complete file and retains
    the newest unique set. Advisory cross-process locking plus synced atomic
    snapshots keep it valid under multiple stdio MCP processes, while entry
    and `HELIXIR_EMBED_CACHE_MAX_BYTES` ceilings bound growth (128 MiB by
    default). Foreign, malformed, truncated, or dimension-mismatched rows are
    invalidated rather than returned.
  - Reasoning chains (`get_chain` with `ChainGuidance`) walk true BFS and
    pick the next hop by **cosine similarity to the query** — the read path
    makes zero generative/reasoning-LLM calls. Chain seeds widen `contextual → full`
    when the contextual window is empty (mature corpora).
- **Modes.** `recent` (~4 h) · `contextual` (~30 d, default) · `deep`
  (~90 d) · `full` (unbounded). Defined in `src/core/search_modes.rs`.
- **Explicit event-time windows and flashbacks.** `time_from`/`time_to`
  constrain seed attention by `valid_from` (falling back to `created_at`).
  Authorized graph neighbors outside the window remain reachable as dated,
  flagged flashbacks under a separate bounded allowance; they never displace
  the requested period's rows. See
  [dataflow](dataflow.md#event-time-windows-and-flashback-projection) and
  [agent userflow](userflow.md#event-time-windows-and-flashbacks).
- **Scopes.** After RBAC has removed every unauthorized row, `personal`
  anchors on the requested owner's `HAS_MEMORY` provenance; `collective` and
  `all` fan out across authorized owners with consensus ranking and controversy
  annotation. Scope never widens the actor's group visibility.
- **`search_by_concept`** — ontology-filtered retrieval gated by
  `INSTANCE_OF Concept(type=<one of 8>)`.
- **`search_reasoning_chain`** — BFS over both directions of the four
  reasoning edges; chain modes `forward / causal / both / deep`. Coverage
  was raised from 40 % to ~95 % when traversal grew from 3 to 8 edge
  directions (v0.3.1).
- **`connect_memories`** — semantic anchors plus an authorized typed path
  answer “how are A and B related?” without exporting the whole graph.
- **`list_memories`** — bounded newest-first audit view with no semantic
  scoring (v0.3.0).
- **`get_memory_graph`** — return a graph view (nodes + edges) around a
  memory or for a user.
- **`search_incomplete_thoughts`** — locate historical pre-RBAC FastThink
  sessions that were persisted as `context_tags=incomplete_thought`.
- **Curated result projection.** Raw sources and extracted atoms are collapsed
  into one representative with `metadata.collapsed`; superseded nodes remain
  reachable but are penalized and labelled with `superseded_by`. Collective
  holder/controversy projection runs only after RBAC filtering.

### 7.3 FastThink (ephemeral working memory)

In-process reasoning scratchpad on `petgraph::stable_graph` — no persistence
until commit. Introduced as the v0.1.1 (`Think_fast`) tag. Tools:
`think_start / think_add / think_recall / think_conclude / think_commit /
think_discard / think_status`. `think_recall` pulls memories from the long-term
store into the live session graph (read-only). Under permanent RBAC, a timeout
fails closed and retains the actor-bound scratchpad for explicit discard or
restart; it cannot infer a safe owner/group for `commit_partial`. Only
historical pre-RBAC interrupted sessions may exist as
`context_tags=incomplete_thought` memories.

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
- `add_memory` runs a two-phase pipeline after resolving one policy domain:
  - Phase 1 — owner-anchored candidates inside that domain.
  - Phase 2 — cross-owner consensus inside the exact same `rbac_scope`;
    identical author nodes share the scoped fingerprint group.
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

These declarations exist in HelixDB but no live Rust product flow writes them.
They are **not current capabilities** and must not be presented as such.
`src/schema_inventory/` now records the explicit status, owner and milestone;
CI rejects any unclassified `N::`, `V::` or `E::` declaration. The admin
control plane renders that same inventory and its bounded live census.

| Surface | Schema artifacts | Implication |
|---|---|---|
| Documentation ingestion | `DocPage`, `DocChunk`, `CodeExample`, `ErrorCode` nodes; `CHUNK_TO_EMBEDDING` edge | Reserved storage for a future document/code pipeline; page/chunk/concept relations are not yet declared. |
| Constraint scoping | `Constraint` node | Reserved policy records; contextual memory already uses live `VALID_IN`. |
| Session tracking | `Session` node; `CREATED_IN` edge | Reserved conversation-scope link; session creation is not wired. |
| Internal concept-graph edges | `IS_A`, `CONCEPT_RELATED_TO` edges | Normalized representation of the **fixed** ontology hierarchy and explicit horizontal links between concepts. See note below. |
| Hierarchical entities | `PART_OF` edge | Entity composition (`engine` PART_OF `car`). |

The separate `Reasoning` node is **deprecated**, not reserved: first-class
`IMPLIES`, `BECAUSE`, `CONTRADICTS` and typed `MEMORY_RELATION` edges are the
authoritative persisted justification model. It stays declared and read-only
until the v2.3.5 backup-first zero-row migration in `data-model.md §2.1`.

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

Most were speculative schema decisions made in earlier releases. An HQL helper
alone does not make a feature active: a declaration needs a live producer, a
consumer, and a DB-verified test before documentation may call it implemented.

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
User or audit history. The CLI and browser projections share the same
graph-backed managers; no UI-owned ACL or registry is permitted.

MCP requests may provide `actor_id` separately from `user_id`. `actor_id` is
the authenticated principal whose grants are evaluated, while `user_id`
remains the memory owner/target. Agents must provide a stable `actor_id`; an
authenticated gateway should populate it explicitly before accepting remote
requests. Transport initialization and ordinary MCP reads never create or
refresh presence. Each root agent or delegated execution announces itself
explicitly through `agent_heartbeat`; each `Agent` row is a concrete execution instance with
an explicit owning `principal_id`; `agent_heartbeat(actor_id, agent_id)` announces
or refreshes it without writing memory, while `add_memory(agent_id=...)` refreshes
the same lease as a convenience. Concurrent sub-agents remain distinct for
farewell and diagnostics but `swarm_status` and the control plane aggregate them
into logical-principal families. Prefix inference applies only to legacy rows in
the MCP and administrator presentation projections and is never an authorization
source. Terminal presence states make one instance inactive immediately and
remain terminal until another explicit heartbeat or attributed write; sibling
instances stay live and the heartbeat window is the
crash/idle fallback. The global-admin web graph may project
`MOIRAI_DERIVED_FROM` edges and their witness memories for audit, but ordinary
agent traversal still omits them and every zero-witness hypothesis is reported
as an integrity violation.
