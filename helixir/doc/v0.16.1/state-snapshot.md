# v0.16.1 release snapshot

> Captured for the `v0.16.1` release on 2026-08-21.

## Patch scope

- No HelixDB schema, query, node, edge, vector, RBAC, dedup, or memory-data
  migration.
- MCP capability surface remains 21 tools, two prompts and three resources.
- The changed boundary is MCP client lifecycle: HTTP-capable clients share one
  host-local gateway; stdio remains a compatibility fallback.

## Local release gates completed

- `cargo test --lib`: 336 passed
- `cargo test --bin helixir`: 14 passed
- `cargo test --all-targets`: passed; live-only suites ignored without their
  explicit external-service gates
- `cargo clippy --all-targets -- -D warnings`: passed
- control-plane unit/build/Playwright release suite: 11 unit and 20 browser
  scenarios passed
- `helixir doctor --json`: ready
- live HTTP MCP initialize, recall and write against the existing graph: passed
- post-restart Codex memory calls: passed
- process invariant after repeated calls: zero `helixir-mcp` children and one
  `helixir gateway run` process

## Publication gates

The immutable tag is published only after release automation builds and
ABI-checks native archives, validates package channels, publishes containers,
and attaches signed release artifacts.
