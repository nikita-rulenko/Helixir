# v0.13.3 — The Onboarding Graph

This patch turns graph-backed RBAC into Helixir's safe onboarding default while
preserving the familiar shared-memory experience. It also extends the guided
installer, exposes a stable administrative CLI contract for the planned UI,
and replaces the remaining oversized Rust source files with cohesive modules.

## RBAC by default, without losing trusted-mode history

Fresh installs and upgrades create the reserved `onboarding` group, grant one
explicit operator global `admin`, enroll detected clients as group
administrators, and migrate existing users as workers. An enrolled writer may
omit `group_id`; other enabled-mode writes must select a concrete group.

Legacy migration now attaches only genuinely pre-RBAC memories: rows with no
stored RBAC scope and no group or dedup-federation edges. Re-running bootstrap
therefore cannot widen already isolated memories. The user registry, active
roles, role history, groups, access edges, and dedup federations all remain in
HelixDB as the single source of truth.

## Administrative CLI and canonical agent contract

`helixir rbac user list/show` and `helixir rbac group
add-user/remove-user` expose stable JSON suitable for automation and the next
UI sprint. Unknown principals cannot self-enroll; membership in `onboarding` is
the admission event, and removal retains audit history.

One versioned Helixir skill is installed for Claude Code, Codex, and Cursor.
The skill, MCP prompt, AGENTS.md, README, and tool descriptions now agree on
actor versus owner identity, default-group routing, group overrides, and the
trusted-network boundary. Config output recursively redacts API keys, tokens,
passwords, secrets, and credentials.

## Installer and model readiness

Guided onboarding discovers supported clients, writes their MCP registration
with backups, provisions the RBAC profile, installs the canonical skill, and
finishes with doctor. NLI is mandatory. Embeddings must be either verified
local Ollama with `nomic-embed-text` or an explicit working OpenAI-compatible
remote endpoint; doctor visibly falls back to Ollama/Nomic when a remote
embedding path is invalid. Cerebras generation is pinned to `gpt-oss-120b`;
Gemma is not selected.

Transactional local HelixDB provisioning remains tracked separately in #108;
until it lands, operators should provision or upgrade the backend with the
documented backup-first HelixDB v2.3.5 flow before running onboarding.

## Maintainable module boundaries

All maintained Rust source files under `src/` are now at most 500 lines. The
former RBAC, configuration, MCP memory, installer, search, FastThink,
orchestration, Hygieia, Lachesis, decision, and extraction monoliths are real
Rust submodules rather than textual includes. `tests/module_budget.rs` scans
the full source tree and rejects future regressions.

## Verification

- 261 library tests, 10 CLI tests, the repository-wide module-budget test, and
  the complete non-ignored test surface pass.
- Formatting, all-target/all-feature Clippy with warnings denied, rustdoc with
  warnings denied, and all-target compilation pass.
- HelixDB CLI v2.3.5 validates and compiles all 163 HQL queries.
- Live enabled-state E2E passes compatibility bootstrap, user enrollment,
  group isolation, dedup federation history, secondary actor binding, and
  preserves enabled enforcement.
- Disposable empty HelixDB instances pass both fresh-install and trusted-mode
  legacy-upgrade bootstrap scenarios.
- Manual MCP stdio smoke registers 21 tools, 2 prompts, and 3 resources; manual
  CLI CRUD confirms enrollment, assignment, revocation history, cleanup, and
  enabled-state persistence.

## Upgrading

Back up the persistent HelixDB volume before changing the schema. With the
pinned HelixDB CLI v2.3.5, run `helix check`, rebuild the `dev` instance image,
recreate the container against the same volume, then replace the Helixir
binaries. Run `helixir doctor --json` and verify `ready: true`; finally restart
Codex, Claude Code, Cursor, gateways, and any other long-lived MCP clients so
they load the new binary and tool schemas.

Use `helixir onboard --legacy-trusted-mode` only when disabled RBAC is an
explicit operational requirement.
