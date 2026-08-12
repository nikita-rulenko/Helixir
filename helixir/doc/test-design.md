# Test design

> _Reflects code as of `v0.14.2`. Last verified: 2026-08-05._

## 1. Stance

Unit-test coverage is **deliberately minimal**. The codebase rewrites fast;
heavy unit-test scaffolding would burn more tokens than the feature work it
guards. The goal is therefore not "high % coverage" but **a small, stable set
of tests that defend the contracts that would silently corrupt the system if
broken**.

This document captures the current state, the contracts worth guarding, and
the gap between the two.

## 2. Current inventory

> _The catalogue below is the original v0.3.1 baseline. As of v0.5.0 the surface
> is far larger — see the current numbers immediately under it._

```
Tests (v0.3.1 baseline):

   ✔ 52 unit tests, all passing                    helixir/src/**/*.rs
   ✔  1 integration test (ignored by default)      helixir/tests/hive_memory_e2e.rs
   ✔  1 bash smoke script                          helixir/tests/test_hive_queries.sh
```

**Current (`v0.14.2` working tree):** 268 library unit tests plus 17 CLI tests
(`cargo test --all-targets`) and **43 HELIX_E2E-gated suites** in
`helixir/tests/*_e2e.rs` (mcp_*, read_path,
clotho/lachesis/atropos, daemon, swarm, nli_antimerge, reasoning_extraction,
negative_inputs, …). A full e2e gate run on cerebras is all-green (0 flaky).
E2E are run by hand (not in CI yet); the manual recipe lives in the suites'
module docs. Run unit tests: `cargo test --lib` from `helixir/`.

The refactor-audit lifecycle coverage includes optional gateway-auth policy,
FastThink generation pinning across hot reload, and the invariant that two
consecutive runtime-generation publications retain one process-owned ingest
worker while swapping its `ToolingManager`.

The issue #89 resource regression is additionally guarded by live, disposable
HelixDB soaks over a cold backup. Primary-key graph/RBAC projections must avoid
the multi-MiB-per-call scan amplification; the remaining upstream v2.3.5
`SearchV` high-water retention must stay inside the managed 3 GiB envelope and
Hygieia must restart the disposable container before OOM without losing the
volume. The one-way RBAC migration may perform only two projected memory-ID
passes. `tools/memprobe.py --dump-to` captures private checksummed arena dumps
and `--analyze-dump` reports structural repetition without emitting recovered
data. The live RBAC cache suite performs 1,000 revision checks and then proves
that a committed grant and revocation invalidate the process cache immediately.

NLI is part of every build. Unit coverage verifies host-variant digest
availability and the contradiction/paraphrase readiness contract; installer
coverage requires NLI download before doctor, and doctor fails closed when the
model is missing. CI runs the same full NLI-enabled surface on Ubuntu and macOS.

`tests/rbac_e2e.rs` is an ignored, enabled-state live contract. It never turns
RBAC off. It covers federated fingerprint equality, isolated-group inequality,
materialized common visibility, viewer denial, detach-with-history, future
isolation, historical in-place update denial, join backfill, cleanup, and the
invariant that RBAC remains enabled.

`tests/rbac_bootstrap_e2e.rs` additionally seeds the pre-v0.14.2 generated
forms (`MEMORY_RELATION/SUPPORTS` and `BECAUSE reasoning_id=lachesis-stitch`),
then verifies that upgrade reifies them under `moirai`, removes ordinary-graph
bridges, preserves unrelated generic relations, restores embedding parity, and
denies the compatibility `default` groupadmin.

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
| LLM factory | `src/llm/factory.rs` | 10 | Provider/fallback construction and the Cerebras `gpt-oss-120b` pin. |
| Helixir client | `src/core/helixir_client.rs` | 3 | Constructor, env constructor, config access. |
| Chunking manager | `src/toolkit/mind_toolbox/chunking/manager.rs` | 3 | `should_chunk`, Cyrillic split, semantic split. |
| Ontology mapper | `src/toolkit/mind_toolbox/ontology/mapper.rs` | 4 | Map preference, map skill, no-match, case-insensitive. |
| Reasoning engine | `src/toolkit/mind_toolbox/reasoning/engine.rs` | 3 | Type→edge name, relation construction, reasoning trail. |
| Temporal scoring | `src/toolkit/mind_toolbox/search/onto_search/temporal.rs` | 2 | Freshness curve, datetime parse. |
| Score combiner | `src/toolkit/mind_toolbox/search/smart_traversal/scoring.rs` | 6 | Cosine (identical/orthogonal/opposite), combined score, rank discrimination, temporal freshness. |
| Utils | `src/utils.rs` | 5 | Safe truncate ASCII/Cyrillic/ellipsis/mixed/shorter. |
| Installer | `src/installer/` | 36 | Fresh and idempotent managed-local/existing-local/remote backend plans, local Ollama/Nomic versus explicit remote embeddings, mandatory NLI, schema backup-before-deploy, permanent RBAC default/onboarding/Moirai coverage, Ollama/model command safety, interrupted API pull retry plus verified inventory, `:latest` equivalence, hardware-aware non-Gemma recommendations, required-step rollback, central TOML/permissions and manifest atomicity, native client command safety, atomic JSON registration, healthy/failed/skipped/degraded doctor reports and malformed-config refusal. |
| CLI onboarding | `src/bin/helixir/` | 15 | RBAC command parsing, deterministic local/remote onboarding flags, recursive secret redaction, real remote embedding probe success/failure, local recovery selection, exact manifest-scoped client readiness, conflict approval, and secret-safe registration diffs. |
| Module budget | `tests/module_budget.rs` | 1 | Recursively rejects every maintained Rust source file under `src/` that exceeds 500 lines. |

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

E2E tests stay opt-in (`#[ignore]` + Make target). A CI workflow on push/PR
runs the following (added in `dev` while closing #5; verify it is on `main`
before relying on it):

```
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo doc --no-deps --document-private-items  # with RUSTDOCFLAGS=-D warnings
cargo test --lib
cargo check --all-targets                 # MSRV 1.88 job pinned in CI
```

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

---

## E2E read-path suites (added with the local-reasoning pre-release)

Two suites over a shared golden query set (10 queries tied to the bench
corpus), both gated by `HELIX_E2E=1` and run with a deliberately **dead LLM
key** — passing proves the read path makes zero LLM calls:

- `tests/read_path_e2e.rs` — library level (`HelixirClient`): hit@5 / MRR
  quality bars (baseline MRR 0.687 after PPR), cold/warm latency, causal
  "why" restoration, collective scope, provenance shape, temporal-window
  isolation, `connect_memories` path shape.
- `tests/mcp_read_e2e.rs` — spawns the real `helixir-mcp` binary and speaks
  stdio JSON-RPC like a real client; same quality bars; measures server boot
  and per-call transport overhead (~0.2 ms vs library).

Run via the commands in the root README §Development. Quality bars are
regression guards set slightly below measured baselines; raising the
baselines is feature work, not test work.

## RBAC coverage

The RBAC unit matrix in `core::rbac` and `core::rbac_compat` covers all six
roles, deny-by-default, worker authorship, viewer write denial, global admin
bypass, cross-group isolation, single-reserved-workspace routing, ambiguous
write denial, and `default` legacy-fingerprint preservation. Installer tests
require all three reserved groups, active migration state, registry coverage, and
legacy-memory coverage before the profile is ready. CLI tests cover stable
parsing without any disable escape, while the repository-wide module
budget is enforced independently by `tests/module_budget.rs`.

The live enabled-state suites keep RBAC on throughout. `rbac_e2e.rs` covers
multi-group isolation and dedup-federation history. `rbac_secondary_e2e.rs`
covers cross-principal FastThink lifecycle denial, pending owner/creator
binding, private outbox reads, and global-admin-only low-level tooling. The CLI
parser and pure policy matrix separately cover all roles, deny-by-default, and
stable management syntax. `rbac_compat_e2e.rs` bootstraps twice, verifies full
legacy-memory coverage, denies pre-onboarding group admission, projects and
retains registry history, validates fresh/legacy convergence, and leaves
enforcement enabled. `HELIXIR_RBAC_ACTOR` is the CLI management identity;
caller-supplied `--actor` impersonation is rejected.
