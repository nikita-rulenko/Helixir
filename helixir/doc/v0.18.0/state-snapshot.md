# v0.18.0 release snapshot

> Captured for the `v0.18.0` release on 2026-08-25.

## Product surface

- Full-host package: `helixir`
- Agent-only package: `helixir-client`
- MCP surface: 23 tools, two prompts and three resources
- Maintained HelixDB: checked-in v2.3.5 fork, immutable multi-platform image,
  exact AGPL source archive and release-bound backend descriptor
- Graph contract: 22 node types, 30 edge types, five vectors and 192 compiled
  HQL queries
- Memory governance: permanent graph-backed RBAC and active charter v1.0
- Default model route: Cerebras `gpt-oss-120b`, no implicit generation fallback
- Operations: reboot-safe shared gateway, global-admin control plane, Moirai,
  Hygieia, backup vault and thin remote-client onboarding

## Release proof

- Rust format/check, strict Clippy, rustdoc, module budget and all-target builds:
  passed
- Helixir deterministic tests: 447 passed
- Independent client tests: 11 passed
- Maintained HelixDB fork tests: 18 passed
- HelixDB mock and memory-boundary tests: 34 Rust plus 45 Python passed
- Control-plane tests: 15 unit plus 24 browser scenarios passed
- HQL compiler: all 192 queries passed with the maintained v2.3.5 CLI
- Complete clean current-schema E2E matrix: passed
- Faithful six-scenario OOM gate: passed with zero OOM kills and zero restarts
- macOS host reboot recovery: one gateway listener; MCP initialize, 23 tools,
  heartbeat and recall passed
- Dogfood production: maintained database image, 0 OOM kills, 0 restarts,
  permanent RBAC and memory recall healthy after migration

The immutable tag workflow rebuilds native binaries, the independent client,
control plane and maintained database for every supported architecture; binds
the server archives to the published database digest/source descriptor; then
publishes and validates Homebrew and signed APT channels.

## Known out of scope

- Transactional Windows package bootstrap remains tracked in issue #131.
- Corpus-wide acyclic supersession, interrupted-restore embedding cardinality
  and long-running federation-consensus monotonicity remain bounded hardening
  opportunities rather than known regressions.
