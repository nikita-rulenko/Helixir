# v0.17.0 release snapshot

> Captured for the `v0.17.0` release on 2026-08-22.

## Product surface

- Full-host package: `helixir`
- Agent-only package: `helixir-client`
- MCP surface: 23 tools, two prompts and three resources
- HelixDB v2.3.5 contract: 22 node types, 30 edge types, five vectors and
  182 compiled queries
- Presence model: durable logical Agent families plus transient root/child
  execution instances

## Migration proof

- The schema transition is additive: `Agent.principal_id`, bounded client
  enrollment and principal-aware presence queries.
- Historical absent/null `principal_id` values are accepted through a safe
  compatibility fallback; new instances persist the authenticated principal.
- Production-shaped migration preserved 4,195 legacy memories and passed
  gateway enrollment, recall and write verification.
- Direct HelixDB access remains a full-host concern; remote clients use only
  the MCP gateway.

## Release gates

- HelixDB CLI v2.3.5 and all 182 HQL queries: passed
- Rust formatting, locked all-target checks, strict Clippy and rustdoc: passed
- Rust tests: 351 server library, 14 CLI and 11 independent-client tests passed
- 500-line module budget: 1/1 passed
- control-plane: 12 Vitest tests, production build and 24 Playwright tests
  passed
- disposable live RBAC/client E2E: 1/1 passed, including concurrent clients
  and group-visibility isolation
- APT: four deterministic amd64/arm64 packages and two signed architecture
  indexes passed repository install/purge-preservation checks
- Homebrew: four server/client formula variants passed rendering, Ruby syntax,
  URL and package-ownership checks; native Intel/Apple Silicon archive installs
  remain enforced by the tag release workflow
- eight shell scripts, three workflow YAML files, documentation, Python,
  Compose, version, secret and diff scans: passed (80 changed files, zero
  secret matches)

The final frozen local diff fingerprint was
`e887dcd4dcb1d501473ffe8206893b52f6beffcf1046de791523d53fedc1b68c`.
The immutable tag workflow independently rebuilds all native archives and
reruns archive-backed APT, Homebrew and Docker publication gates.

## Known open work

- Issue #157 remains open at priority/P0 for the wider declared-schema versus
  live-semantics reconciliation. v0.17.0 ships only the additive, exercised
  client and presence portion; it does not close or downgrade that issue.
