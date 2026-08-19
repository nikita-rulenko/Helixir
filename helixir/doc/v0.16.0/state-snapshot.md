# v0.16.0 release snapshot

> Captured for the `v0.16.0` release on 2026-08-19.

## Product surface

- Rust 2024 workspace with three native binaries: `helixir`, `helixir-mcp` and
  `helixir-deploy`.
- HelixDB v2.3.5 contract: 22 node types, 30 edge types and 180 named HQL
  queries. The v3/hyperscale engine remains intentionally unsupported.
- MCP surface: 21 tools, two prompts and three resources.
- Permanent graph-backed RBAC with reserved `default`, `onboarding` and
  membership-free `moirai` workspaces.
- Global-admin-only web control plane with installation, observability, RBAC,
  bounded memory graph, Moirai, Hygieia and Stewardship surfaces.
- The release workflow publishes ABI-gated native archives, a Homebrew formula,
  a signed multi-version APT repository and multi-architecture containers from
  the same native artifacts.
- Evergreen product/engineering docs, integration templates, the canonical
  Agent Skill and embedded prompt are reconciled with the current schema,
  permanent RBAC, retrieval and installer contracts.
- The GitHub landing page uses progressive disclosure and native Mermaid;
  detailed installation and operations references live under `helixir/doc/`.

## Local release gates completed

- `cargo check --all-targets`
- strict `cargo clippy --all-targets -- -D warnings`
- complete `cargo test --all-targets` suite: 348 unit/CLI tests passed; live-only
  integration cases remained explicitly ignored in the ordinary suite
- Helix CLI v2.3.5 syntax, query compilation (180 queries) and Cargo check
- frontend unit tests, production Vite build and 20 Playwright journeys across
  Chromium, Firefox, WebKit and the mobile viewport
- live native browser smoke against the local HelixDB, including the redacted
  settings view and review-before-write dialog without mutating production
  configuration
- atomic installation of the v0.16.0 release binary set, a fully green
  `helixir doctor --json`, RBAC/group/swarm reads, and a direct MCP initialize,
  21-tool enumeration and real `search_memory` call against the installed binary

## Tag-time publication gates

The immutable tag is considered published only after release automation:

- builds and ABI-checks all native archives;
- validates clean Homebrew and Debian/Ubuntu installs;
- verifies the APT signing fingerprint and an upgrade from the preceding
  indexed version;
- publishes both container architectures and the `latest` manifest;
- attaches archives, checksums and Debian packages to the GitHub release.
