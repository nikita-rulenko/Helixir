# v0.17.2 release snapshot

> Captured for the `v0.17.2` release on 2026-08-23.

## Product surface

- Full-host package: `helixir`
- Agent-only package: `helixir-client`
- MCP surface: 23 tools, two prompts and three resources
- HelixDB v2.3.5 contract: 22 node types, 30 edge types, five vectors and
  189 compiled queries
- Memory governance: active charter v1.0 with Rust and atomic HQL guards
- Default generation route: Cerebras `gpt-oss-120b`, no implicit fallback
- Client placement: CLI plus global-admin control-plane onboarding registry

## Release proof

- Rust format/check, strict Clippy, rustdoc and all-target builds: passed
- Server and independent-client unit/integration suites: passed
- HelixDB CLI v2.3.5 compilation of all 189 queries: passed
- Control-plane Vitest, TypeScript, production build and four-browser
  Playwright matrix: passed
- Documentation lint, module budget, diff and package metadata checks: passed
- Docker client-gate safety preflight, including in-command daemon loss:
  passed without creating containers
- Disposable APT/client/RBAC/charter gate: pending the immutable candidate run
- Native Homebrew package matrix: pending the immutable candidate run
- Dogfood backup rehearsal and exact runtime/schema reconciliation: pending
  the immutable candidate run

The pending rows above are release blockers. This snapshot must be amended with
their workflow run and dogfood evidence before the immutable tag is published.

## Known out of scope

- Transactional Windows package bootstrap remains tracked separately in issue
  #131.
