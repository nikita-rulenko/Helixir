# v0.17.1 release snapshot

> Captured for the `v0.17.1` release on 2026-08-23.

## Product surface

- Full-host package: `helixir`
- Agent-only package: `helixir-client`
- MCP surface: 23 tools, two prompts and three resources
- HelixDB v2.3.5 contract: 22 node types, 30 edge types, five vectors and
  185 compiled queries
- Physical lifecycle ledger: 57 classified declarations — 40 active,
  16 reserved and one deprecated
- Client placement: graph-backed, global-admin-only and resumable through
  `helixir rbac user onboard`

## Schema proof

- Unit/CI drift checks parse all `N::`, `V::` and `E::` declarations directly
  from `schema.hx` and compare them with the Rust inventory.
- Every active entry carries producer, consumer and E2E evidence; every
  reserved entry has an owner and milestone; the deprecated `Reasoning` node
  has a backup-first removal plan and remains readable in v0.17.1.
- A disposable live HelixDB instance counted all 57 declarations through the
  three bounded census queries: 40 active, 16 reserved, one deprecated, with
  zero `Reasoning` rows.
- Documentation and the control plane project the same inventory used by the
  release gate; census failures remain explicit diagnostics.

## Release gates

- HelixDB CLI v2.3.5 and all 185 HQL queries: passed
- Rust formatting, all-target tests and strict Clippy: passed
- Rust tests: 358 server library and 14 CLI tests passed
- Independent client: 11 tests passed
- 500-line module budget: passed
- Control plane: 13 Vitest tests and production build passed
- Disposable live schema census: passed, with all temporary database resources
  removed afterwards
- Documentation lint and diff checks: passed

The immutable tag workflow independently rebuilds native server/client
archives, mandatory NLI bundles and multi-architecture containers, then reruns
the remote-client, Homebrew, APT and distribution gates before publishing the
GitHub release and package channels.

## Known open work

- Transactional Windows package bootstrap remains tracked separately in
  issue #131 and is not part of this patch.
