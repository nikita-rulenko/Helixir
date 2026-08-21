# Test design

> _Reflects code as of `v0.16.0`. Last verified: 2026-08-20._

## 1. Stance

Coverage is **contract-driven**, not percentage-driven. Pure policy,
validation, projection and orchestration code is exercised with fast unit
tests; HelixDB snapshot behavior, HQL, MCP transport, model adapters, browser
flows and package installation use progressively heavier integration gates.
The goal is evidence at the boundary that can actually fail, with special
weight on contracts that could corrupt memory, leak RBAC scope, expose a secret
or make recovery destructive.

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

**Current (`v0.16.0`):** 333 library unit tests plus CLI tests
(`cargo test --all-targets`) and **43 HELIX_E2E-gated test files** in
`helixir/tests/*_e2e.rs` (mcp_*, read_path,
clotho/lachesis/atropos, daemon, swarm, nli_antimerge, reasoning_extraction,
negative_inputs, …). E2E are opt-in and are not implied by an ordinary green
`cargo test`; record the provider, database fixture and date whenever claiming
a live run. The manual recipes live in the suites' module docs. Run unit tests:
`cargo test --lib` from `helixir/`.

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

The persistent embedding-cache contract is covered separately: provider,
endpoint, revision, dimension and epoch produce isolated namespaces; startup
keeps the newest unique entry/byte-bounded set; malformed, foreign and
wrong-dimension rows fail safe; durable clear survives restart; an orphaned
temporary snapshot cannot replace the last valid cache; and concurrent store
handles cannot interleave JSONL records during append or compaction. Diagnostics
assert hits, misses, bytes, compactions and invalidations without exposing raw
memory text or vectors.

Package-distribution CI renders the Homebrew formula from release-style
checksums, validates Ruby syntax, builds byte-identical Debian packages twice,
checks package metadata and runtime-resource layout, builds both Debian
architectures, assembles an ephemeral APT repository, and verifies its
`InRelease` signature while rejecting tampered metadata. The contract installs
an older indexed version, upgrades it through APT, and proves that purge keeps
user state. Release Linux binaries use an Ubuntu 22.04 build container and a
symbol-version gate so the native archive and derived packages keep a Debian
12/Ubuntu LTS-compatible glibc baseline; release validation clean-installs both
architectures on both supported distribution families before publication.
Container publication reuses those same ABI-gated Linux archives. A native
runner for each architecture packages both runtime images without compiling
Rust in Docker or under QEMU; the architecture-specific NLI model and the
single shared frontend build are workflow artifacts. Per-target/per-architecture
BuildKit GHA scopes retain the small immutable packaging layers across warm
releases. Only after both architecture jobs succeed does the manifest job move
the immutable release tag and `latest` together, keeping the post-native-build
container budget below 20 minutes.

The v0.15 control plane adds a separate browser contract. Every API route is
covered by the same persistent browser-token and graph-backed global-admin
middleware. Unit contracts additionally reject mismatched Origin/Host pairs and
cross-site fetch metadata while keeping bearer-authenticated non-browser clients
possible; the router caps JSON bodies at 1 MiB and returns stable problem codes
without projecting internal RBAC/planner errors;
projection parsers tolerate HelixDB's wrapped/null response shapes. Automated
Playwright gates cover Chromium, Firefox, WebKit and a mobile Chromium viewport,
the complete plan/apply/verify journey, fail-closed admin isolation, WCAG
serious/critical findings, responsiveness and bounded polling. Manual live
browser smoke additionally covers all six navigation surfaces, searchable/paginated RBAC
registries and mutations, reserved-workspace guards, graph identity/workspace
filters, node inspection and zoom controls, group/author clusters, admin-only
`MOIRAI_DERIVED_FROM` witness rendering, expandable evidence ledgers, the
Hygieia resource/journal bridge, RBAC permission simulation, offline-presence pruning,
typed Moirai/Hygieia lifecycle operations, remote database/embedding choices, and the
installer mutation preview. The production
container is also checked as non-root, read-only, capability-free, and
`no-new-privileges`. Token tests prove private atomic creation, strict container
failure for absent or malformed secrets, stable browser reuse across requests,
authenticated-URL capture, and the explicit HTTP 401 recovery signal. Live smoke
must additionally reload the same browser tab after two container restarts.
Installer-operation coverage injects failure into every required action, proves rollback,
replays SSE strictly after its cursor, rejects a changed-plan resume, redacts
sensitive event/report detail on disk, and reopens a running journal as an
explicit resumable interruption. Shared-installer tests additionally prove that
CLI and browser adapters converge on the same `InstallerService`, plan/debug
projections never disclose provider secrets, conflicting MCP registrations need
explicit replacement consent, and every concrete executor module remains within
the 500-line source budget. Stewardship tests additionally cover write-only
secret rendering, allowlisted settings patches, complete cross-field validation,
vault path confinement, exact restore confirmation, frontend review-before-write,
and disabled recovery actions for non-managed databases.

`tools/control_plane_soak.py` performs authenticated polling against the real
overview, access, memory-field, Moirai and Hygieia projections while sampling
the container working set. The release budget is at most 96 MiB growth; the
v0.15.0 release run completed 100 live reads with 0.3 MiB peak growth.

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

### Representative unit contracts

Exact per-module counts are intentionally omitted: they drift without saying
whether an architectural boundary is protected. The repository-wide totals
above come from the current runner; this table maps stable contracts to their
owners.

| Area | File | Contract focus |
|---|---|---|
| Config | `src/core/config.rs` | Environment layering, defaults and validation. |
| Search modes | `src/core/search_modes.rs` | Default, parsing and token-cost estimates. |
| Event-time windows | `src/core/time_window.rs` | Inclusive RFC3339/date bounds, open sides and malformed-input behavior. |
| Search result projection | `src/toolkit/mind_toolbox/search/dispatch*` | Separate flashback allowance, raw/atom family collapse and superseded-history labelling. |
| Levels (deploy ordering) | `src/core/levels/utils.rs` | Deployment order, dependencies and accumulated schema. |
| Event bus | `src/core/events/bus.rs` | Handler delivery. |
| DB client | `src/db/client.rs` | Explicit and environment-backed construction. |
| LLM decision | `src/llm/decision/engine.rs` | Builder and scoped cross-owner decision branches. |
| LLM extractor | `src/llm/extractor.rs` | Typed extraction-result serialization. |
| LLM factory | `src/llm/factory.rs` | Provider/fallback construction and the Cerebras `gpt-oss-120b` pin. |
| Helixir client | `src/core/helixir_client/` | Construction, configuration and administrative facade boundaries. |
| Chunking manager | `src/toolkit/mind_toolbox/chunking/manager.rs` | Chunk eligibility and multilingual splitting. |
| Ontology mapper | `src/toolkit/mind_toolbox/ontology/mapper.rs` | Fixed-type mapping and normalization. |
| Reasoning engine | `src/toolkit/mind_toolbox/reasoning/engine.rs` | Semantic relation mapping and reasoning trails. |
| Temporal scoring | `src/toolkit/mind_toolbox/search/onto_search/temporal.rs` | Freshness and event-time parsing. |
| Score combiner | `src/toolkit/mind_toolbox/search/smart_traversal/scoring.rs` | Cosine, combined rank and temporal freshness. |
| Utils | `src/utils.rs` | Unicode-safe truncation. |
| Installer and stewardship | `src/installer/` | Shared CLI/browser service, three backend topologies, mandatory models, transaction journal, secret-safe projections, settings and guarded backup/restore. |
| CLI onboarding | `src/bin/helixir/` | Stable parsing, RBAC operator reuse, remote-embedding probes, registration conflicts and redaction. |
| Module budget | `tests/module_budget.rs` | Every maintained Rust source under `src/` stays at or below 500 lines. |

### Integration / E2E

- `helixir/tests/hive_memory_e2e.rs::hive_cross_user_collective_link_e2e`
  — marked `#[ignore]`. Runs only with `make test-e2e-hive` and requires:
  live HelixDB, real LLM API key, real embedding API key.
  Asserts: two owners in one authorized RBAC group/federation contribute to
  one scoped consensus family (`user_count ≥ 2`), while isolated groups do not.
- `helixir/tests/test_hive_queries.sh` — bash script poking HelixDB queries
  directly. Not invoked from `make test`.

## 3. Contract map: what is guarded vs. what isn't

The current suite protects every architectural boundary with a different kind
of evidence rather than an invented coverage percentage:

| Boundary | Primary evidence |
|---|---|
| Process entry and MCP transport | `mcp_full_surface_e2e`, multi-consumer and concurrent MCP suites spawn real `helixir-mcp` processes and exercise the registered surface through JSON-RPC. |
| MCP schema and resources | handler/parameter unit tests plus full-surface e2e keep tool names, required arguments, config/rules resources and wire responses aligned. |
| `HelixirClient` / `ToolingManager` | focused unit tests protect deterministic policy, scoring and projection code; live suites prove persistence and graph behavior against HelixDB. |
| HelixDB graph contract | schema-version, edge-verification, RBAC/bootstrap, temporal, contradiction, ontology and Moirai suites inspect persisted outcomes rather than trusting HTTP status alone. |
| External model adapters | mandatory local NLI readiness is deterministic; real LLM/embedding behavior remains opt-in behind `HELIX_E2E=1`. |
| Installer and host mutations | typed-plan unit tests, failure injection, durable-operation replay, command-boundary tests and manual disposable-host smoke. |
| Browser control plane | API authorization/unit contracts, frontend component tests, Playwright release gates, container hardening checks and live browser smoke. |
| Distribution | reproducible package construction, ABI/symbol gates, signed ephemeral APT metadata and clean-install/upgrade matrices. |

### Remaining data-integrity risks

The high-value live suites cover embedding parity during RBAC/Moirai migration,
persisted reasoning edges, dedup/federation isolation, contradiction resolution,
supersession demotion and ontology classification. The remaining gaps are
bounded properties that require a deliberately corrupted fixture rather than a
normal product flow:

1. A global graph audit that proves `SUPERSEDES` is acyclic across an arbitrary
   imported database.
2. A corpus-wide assertion that every non-deleted memory has exactly one current
   embedding of the configured dimension after an interrupted external restore.
3. A long-running monotonicity audit over consensus holder counts during
   concurrent federation attach/detach and supersession.

These are release-hardening opportunities, not known shipped regressions. The
managed restore path reduces their blast radius by probing the live schema and
rolling back to a fresh safety snapshot when recovery is incompatible.

## 4. Test strategy going forward

### Tier 1 — keep
Pure-function tests in `mind_toolbox` (scoring, temporal, mapper) and
`llm/decision` (builders). They are cheap, fast, and they encode invariants
that change only with deliberate decisions.

### Tier 2 — add (small, deliberate)

Add one contract at the narrowest stable boundary whenever a regression is
found. Prefer pure projection/validation tests and typed fake executors; use a
real disposable HelixDB only for behavior that depends on HQL, snapshot
visibility, vector search or graph traversal. Never mock an external boundary
and then cite the mock as proof that the integration works.

### Tier 3 — gate

LLM/HelixDB E2E tests stay opt-in (`#[ignore]`, `HELIX_E2E=1` and dedicated
Make targets). Push/PR CI runs the deterministic native gates:

```
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo doc --no-deps --document-private-items  # with RUSTDOCFLAGS=-D warnings
cargo test --lib
cargo check --all-targets                 # MSRV 1.88 job pinned in CI
```

### Tier 4 — refuse

Things that should not be asserted as brittle snapshots:

- The exact prompt text sent to LLMs. It changes constantly; a snapshot
  test would generate noise every refactor.
- HelixDB's own behavior. It is an external dependency.
- Concrete embedding values. They change with model versions.
- Pixel-identical UI rendering. Test semantics, accessibility, responsive
  layout and visual regressions at deliberate checkpoints instead.

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
generation-LLM key** — passing proves the read path does not call the reasoning
model; its embedding endpoint remains live:

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
