# v0.16.0 release-readiness snapshot (unreleased)

> Captured from local branch `codex/local-readiness-v016` on 2026-08-19.
> This file becomes a frozen release snapshot only when the v0.16.0 tag is cut.

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
- Homebrew/APT distribution workflows and artifact-reuse container publishing
  are implemented locally; publication is still a release action.
- Evergreen product/engineering docs, integration templates, the canonical
  Agent Skill and embedded prompt are reconciled with the current schema,
  permanent RBAC, retrieval and installer contracts.

## Local release gates completed

- `cargo check --all-targets`
- strict `cargo clippy --all-targets -- -D warnings`
- control-plane, settings, backup-vault and module-budget Rust tests
- frontend unit tests and production Vite build
- live native browser smoke against the local HelixDB, including the redacted
  settings view and review-before-write dialog without mutating production
  configuration

## Gates still required before tagging

- Run the complete Rust and frontend/browser release suites from a clean tree.
- Build/install the release binaries locally, restart the MCP client, and run
  the complete live MCP/CLI smoke against the installed artifact.
- Let release CI build ABI-gated archives, validate clean Homebrew/APT installs,
  publish both container architectures, then move the immutable tag and
  `latest` manifest.
- Verify the published package repository signatures and perform one clean
  install plus upgrade from the preceding indexed version.
