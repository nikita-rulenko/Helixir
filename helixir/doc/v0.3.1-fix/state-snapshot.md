# v0.3.1-fix — state snapshot

> Captured: 2026-05-12. This file is a **frozen snapshot**; do not edit after
> the next tag is cut. For live state, run `gh issue list ...`.

## Codebase metrics

| Metric | Value | Source |
|---|---|---|
| Rust source LOC | 22 210 | `wc -l helixir/src/**/*.rs` |
| HQL queries | ~100 | `helixir/schema/queries.hx` |
| Schema nodes | 15 | `helixir/schema/schema.hx` |
| Schema edges | 33 (24 active + 9 reserved) | same |
| MCP tools registered | 14 | `src/mcp/server.rs` |
| MCP prompts | 2 | `src/mcp/server.rs` |
| MCP resources | 2 | `src/mcp/server.rs` |
| Unit tests | 52 (all green) | `cargo test --lib` |
| E2E tests | 1 (`#[ignore]`) | `helixir/tests/hive_memory_e2e.rs` |
| `cargo clippy` warnings | 15 | mostly `too_many_arguments` |
| Default branch | `main` | github |
| HEAD at snapshot | detached on `v0.3.1-fix` | `git status -sb` |

## Build / health checks

| Check | Result |
|---|---|
| `cargo check` (default features) | passes in ~8 s |
| `cargo build --release` (cold) | not measured at this snapshot |
| `cargo test --lib` | 52 passed / 0 failed / 0 ignored in 0.10 s |
| `cargo clippy --message-format=short` | passes, **15 warnings** |
| Release workflow on tag push | **expected to fail** at "Install Rust" step (action name typo) |
| `cargo fmt --all -- --check` | not run in CI; format state unknown |

## Open issues, by priority

### Critical (priority/P0) — 3

| # | Labels | Title |
|---|---|---|
| 5 | bug, ci | CI: release workflow uses non-existent action; no CI on push/PR |
| 6 | tech-debt | lib.rs: blanket `#![allow(...)]` silences compiler |
| 7 | bug, config | Embeddings: OpenAI-compat wiring uses Ollama URL as default — fix lost when PR #3 was closed |

### High (priority/P1) — 4

| # | Labels | Title |
|---|---|---|
| 8 | docs, tech-debt, config | Version drift: Rust MSRV, crate version, default LLM model declared in 4–5 places each |
| 9 | tech-debt, architecture | Architecture: duplicate chunking modules, smart_traversal_v2 leftover, add_pipeline god-object |
| 10 | tech-debt, config | Config: half of HelixirConfig fields unreachable from env, db client ignores configured retries |
| 11 | tech-debt, infra | Cargo.lock policy: locks gitignored — non-reproducible binary builds |

### Medium (priority/P2) — 3

| # | Labels | Title |
|---|---|---|
| 12 | tech-debt, data-model | Schema: booleans as I64, time drift String/Date, denormalized parent_id, JSON-in-string |
| 13 | security, infra | Deploy: unpinned `:latest` images, obsolete compose schema, MCP-stdio misconfig in compose |
| 14 | bug, docs | Docs sync: README structure stale; MCP config resource omits 2 tools; `list_memories` filters after limit |

### Low (priority/P3) — 1

| # | Labels | Title |
|---|---|---|
| 15 | tech-debt, infra | Housekeeping: snapshots dir, unsafe env in tests, CLI parsing, log emoji, ansible chaos, CI cache |

## Resolved at or before this tag

| Tag | Major fixes |
|---|---|
| v0.3.1-fix | Three independent root causes of `relations_created: 0`; full release notes via `gh release view v0.3.1-fix`. |
| v0.3.1 | Reasoning chain hit rate raised 40% → ~95%; extraction retry+fallback; coherence validation; version strings synced (except the MCP resource — see #8). |
| v0.3.0 | Real cosine re-ranking; `list_memories`; raw source storage. |
| v0.2.x line | Hive Memory; 8 ontology types; 33 edge types; performance refactor (2.9× faster `add_memory`). |

## Released artifacts

`gh release list -R nikita-rulenko/Helixir` shows 9 releases. Build artifacts
are produced by `helixir/.github/workflows/release.yml` for 5 targets:
`{linux,macos}-{x86_64,arm64}` + `windows-x86_64`, plus a Docker image push
to `helixir/helixir:latest` and `:<tag>`. As of this snapshot, the workflow
is expected to fail because of issue #5.

## Notes for the next release

If you are cutting v0.3.2 / v0.4.0:

1. Re-run all checks in the "Build / health checks" table above and pin
   the deltas in the new `<version>/state-snapshot.md`.
2. Walk the open-issues lists; for any closed in this cycle, put them in
   `<version>/notes.md` under "Resolved" with the commit/PR id.
3. If the schema changed, document the migration in the new
   `<version>/notes.md`.
4. Diff the four project-level docs (`architecture`, `data-model`,
   `dataflow`, `userflow`) against the new code; bump the
   "Last verified" header on each.
