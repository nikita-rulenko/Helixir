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
