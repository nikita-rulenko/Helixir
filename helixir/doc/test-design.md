# Test design

> _Reflects code as of `v0.3.1-fix`. Last verified: 2026-05-12._

## 1. Stance

Unit-test coverage is **deliberately minimal**. The codebase rewrites fast;
heavy unit-test scaffolding would burn more tokens than the feature work it
guards. The goal is therefore not "high % coverage" but **a small, stable set
of tests that defend the contracts that would silently corrupt the system if
broken**.

This document captures the current state, the contracts worth guarding, and
the gap between the two.

## 2. Current inventory

```
Tests today (cargo test, no E2E):

   ✔ 52 unit tests, all passing                    helixir/src/**/*.rs
   ✔  1 integration test (ignored by default)      helixir/tests/hive_memory_e2e.rs
   ✔  1 bash smoke script                          helixir/tests/test_hive_queries.sh
   ✘  0 CI runs on push / PR (release-only)        see issue #5
```

Run: `cargo test --lib` from `helixir/`.

### Unit-test distribution

| Area | File | Tests | What they actually check |
|---|---|---|---|
| Config | `src/core/config.rs` | 2 | `from_env` reads `HELIX_LLM_BASE_URL`; default has no base url. |
| Search modes | `src/core/search_modes.rs` | 3 | Default, parse-from-str, token-cost estimate. |
| Levels (deploy ordering) | `src/core/levels/utils.rs` | 3 | Deployment order, dependencies, accumulated schema. |
| Velocity metrics | `src/core/velocity/metrics.rs` | 3 | Score min/max/zero edge cases. |
| Event bus | `src/core/events/bus.rs` | 1 | `emit` invokes the handler once. |
| DB client | `src/db/client.rs` | 2 | Constructor works; `from_env` constructor works. |
| LLM decision | `src/llm/decision/engine.rs` | 6 | Builder constructors, cross-user prompt branches. |
| LLM extractor | `src/llm/extractor.rs` | 1 | `ExtractionResult` serializes round-trip. |
| LLM factory | `src/llm/factory.rs` | 4 | Constructs cerebras/ollama; unknown provider panics. |
| Helixir client | `src/core/helixir_client.rs` | 3 | Constructor, env constructor, config access. |
| Chunking manager | `src/toolkit/mind_toolbox/chunking/manager.rs` | 3 | `should_chunk`, Cyrillic split, semantic split. |
| Ontology mapper | `src/toolkit/mind_toolbox/ontology/mapper.rs` | 4 | Map preference, map skill, no-match, case-insensitive. |
| Reasoning engine | `src/toolkit/mind_toolbox/reasoning/engine.rs` | 3 | Type→edge name, relation construction, reasoning trail. |
| Temporal scoring | `src/toolkit/mind_toolbox/search/onto_search/temporal.rs` | 2 | Freshness curve, datetime parse. |
| Score combiner | `src/toolkit/mind_toolbox/search/smart_traversal_v2/scoring.rs` | 6 | Cosine (identical/orthogonal/opposite), combined score, rank discrimination, temporal freshness. |
| Utils | `src/utils.rs` | 5 | Safe truncate ASCII/Cyrillic/ellipsis/mixed/shorter. |

### Integration / E2E

- `helixir/tests/hive_memory_e2e.rs::hive_cross_user_collective_link_e2e`
  — marked `#[ignore]`. Runs only with `make test-e2e-hive` and requires:
  live HelixDB, real LLM API key, real embedding API key.
  Asserts: adding the same fact for two user_ids yields `user_count ≥ 2` on
  the first memory.
- `helixir/tests/test_hive_queries.sh` — bash script poking HelixDB queries
  directly. Not invoked from `make test`.

## 3. Contract map: what is guarded vs. what isn't

The five layers from `architecture.md`, scored by how protected they are
against silent regression:

```
L5  process entry          ░░░░░░░░░░  0%  no smoke test that `helixir-mcp` starts
L4  MCP surface            ░░░░░░░░░░  0%  no test that tool names match registered
L3  HelixirClient facade   ███░░░░░░░ 30%  constructor & config tests only
L2  ToolingManager         ██░░░░░░░░ 20%  unit tests on isolated managers
L1  external adapters      ███░░░░░░░ 30%  unit tests on factory/decision/scoring
                                          E2E only via the ignored hive test
```

### Contracts that would corrupt data if violated

These are the assertions that **must** continue to hold or the persisted store
gets wrong. They are exactly where to focus the (minimal) test budget.

1. **Memory persistence ↔ embedding parity.** Every persisted `Memory` has
   exactly one `HAS_EMBEDDING` edge to a `MemoryEmbedding` whose model name
   matches `embedding_model`. Today: not checked.
2. **SUPERSEDES acyclic.** Following SUPERSEDES backwards from any Memory
   eventually reaches a non-superseded Memory. Today: not checked.
3. **`user_count` monotone.** Across `add_memory` calls for any
   `memory_id`, `user_count` never decreases. Today: only checked via the
   E2E hive test, and only for the 1→2 transition.
4. **Decision engine never returns ADD when score ≥ exact_duplicate_score
   (0.98).** Today: no test wires the engine to a real similarity input.
5. **`HAS_HISTORY` on every UPDATE/SUPERSEDE/DELETE.** Today: not checked.
6. **Ontology classifier never assigns a non-leaf concept.** Today: not
   checked.
7. **Soft delete leaves `is_deleted=1` and `deleted_at != ""`.** Today: not
   checked.

### Contracts that would corrupt behavior (not data)

These cause user-visible incorrectness but not stored garbage. Lower
priority but still cheap to add:

- `list_memories(memory_type=X, limit=N)` returns ≤ N items of type X.
  (Currently broken — issue #14 — so a test would pin the regression.)
- `search_memory(mode=recent)` excludes memories older than ~4h.
- `read_resource("config://helixir").version == env!("CARGO_PKG_VERSION")`.
- `read_resource("config://helixir").tools` matches the registered tool set.

## 4. Test strategy going forward

### Tier 1 — keep
Pure-function tests in `mind_toolbox` (scoring, temporal, mapper) and
`llm/decision` (builders). They are cheap, fast, and they encode invariants
that change only with deliberate decisions.

### Tier 2 — add (small, deliberate)

Add **one fake-backed contract test** per L4/L3/L2 boundary, replacing the
HelixDB client with an in-memory fake. This protects against silent contract
drift without paying the cost of full mocks.

| Test | Replaces | Defends |
|---|---|---|
| `mcp_tool_list_matches_router` | Manually maintained list in `read_resource` | Issue #14 root cause |
| `version_resource_matches_cargo_pkg` | Hardcoded `"0.3.0"` string | Issue #8 root cause |
| `decision_engine_never_adds_on_exact_dup` | Brittle prompt regression | Data integrity invariant #4 |
| `soft_delete_sets_flag_and_timestamp` | Bare CRUD path | Invariant #7 |
| `supersede_creates_edge_and_history` | Bare graph helpers | Invariants #2, #5 |
| `list_memories_filters_in_query` | Issue #14 fix verification | Behavior-level |

### Tier 3 — gate

E2E tests stay opt-in (`#[ignore]` + Make target). What we **need** before
v0.4 is a CI workflow on push/PR that runs:

```
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --lib
```

Tracked in issue #5. Without this, all the tier-2 tests above can regress
silently on the next merge.

### Tier 4 — refuse

Things that should NOT be tested at this stage:

- The exact prompt text sent to LLMs. It changes constantly; a snapshot
  test would generate noise every refactor.
- HelixDB's own behavior. It is an external dependency.
- Concrete embedding values. They change with model versions.
- UI / output formatting strings.

## 5. Open testing-related issues

`gh issue list -R nikita-rulenko/Helixir --label tests --state open`

(There may be no `tests` label yet. The relevant items live under the
priority/P0–P3 + tech-debt tags; see `<version>/state-snapshot.md` for the
list of open testing-adjacent issues at this release.)

## 6. How to add a test (the lazy way)

1. Pick one invariant from §3 not yet covered.
2. Write the test in the same module as the code it guards.
3. Keep the test deterministic — no live HelixDB, no real LLM call.
4. If the invariant requires a backing store, build the smallest fake
   `HelixClient`-shaped struct that returns the data you need. Do not
   reach for `mockall` / `mockito` unless the test pays for itself.
5. Run `cargo test --lib` — it must stay under 5 seconds total.

If the test takes more than 30 lines to write, the invariant is probably
better defended at the schema or type-system level. Stop and refactor
instead.
