# v0.14.0 — The Governed Hive

This release turns graph-backed RBAC into Helixir's permanent operating model while
preserving the familiar shared-memory experience. It also extends the guided
installer, exposes a stable administrative CLI contract for the planned UI,
and replaces the remaining oversized Rust source files with cohesive modules.

## Permanent RBAC without losing legacy shared history

Fresh installs and upgrades create two reserved workspaces: `default` preserves
the historical trusted-peer data plane, while `onboarding` admits newly
discovered principals before an administrator assigns their long-term groups.
One explicit operator receives global `admin`; selected trusted clients receive
equal `groupadmin` access to `default`; new principals enter `onboarding` as
workers. A writer with exactly one writable reserved workspace may omit
`group_id`; ambiguous writes fail closed.

The one-way migration moves genuinely pre-RBAC memories into `default`, records
`pending → migrating → active` checkpoints in HelixDB, and resumes idempotently
after interruption. It never disables enforcement or reclassifies memories
created after activation. The user registry, active roles, role history,
groups, access edges, dedup federations, and migration state all remain in
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

Guided onboarding discovers supported clients, writes minimal secret-free MCP
registrations with timestamped backups and exact post-write verification,
provisions the RBAC profile, installs the canonical skill, and finishes with a
real MCP/backend/model/client doctor gate. NLI is mandatory. Embeddings must be
either verified local Ollama with `nomic-embed-text` or an explicit working
OpenAI-compatible remote endpoint; doctor visibly falls back to Ollama/Nomic
when a remote embedding path is invalid. Cerebras generation is pinned to
`gpt-oss-120b`; Gemma is not selected.

The installer now distinguishes a Helixir-managed local HelixDB, an existing
separately managed local database, and a remote database. Managed schema
transitions use HelixDB CLI v2.3.5, a recoverable cold volume backup, the same
persistent volume, read-only schema verification, and restoration on failure.
The product default remains port 6969; an explicitly detected endpoint such as
the owner's 6970 is preserved exactly. Source and checksummed release installs
use immutable version directories and restore the previous `current` pointer
when onboarding fails.

## Maintainable module boundaries

All maintained Rust source files under `src/` are now at most 500 lines. The
former RBAC, configuration, MCP memory, installer, search, FastThink,
orchestration, Hygieia, Lachesis, decision, and extraction monoliths are real
Rust submodules rather than textual includes. `tests/module_budget.rs` scans
the full source tree and rejects future regressions.

## Bounded HelixDB memory under onboarding load

Issue #89 was traced with a live anonymous-memory dump rather than RSS alone.
The hot arenas contained repeated decoded `Memory` records from full HQL scans;
HelixDB v2.3.5 retains arena-backed scan material across requests. Primary-key
projections remove the severe label-scan multiplier, but the upstream `SearchV`
primitive still retains a smaller request high-water mark. Managed containers
therefore run one visible HelixDB core with immediate `mimalloc` purging, a
3 GiB hard limit, and Hygieia's supervised pre-OOM restart as the operational
envelope. The restart preserves the LMDB volume and now waits for the compiled
schema endpoint before reporting recovery or writing an alert. Login-service
installation carries the manifest's RBAC operator and a deterministic Docker
search path into launchd/systemd, so fail-closed authorization cannot turn the
watchdog into a restart loop after login.

The RBAC transition now projects only memory IDs, performs at most two complete
label scans during the one-way legacy cutover, and records `active`
only after the second pass verifies coverage. Normal `doctor`, repeat onboarding,
and idempotent bootstrap trust that durable checkpoint instead of decoding the
entire memory graph again. `tools/memprobe.py` can also capture private,
checksummed zstd heap dumps and report structural repetition without printing
recovered application content.

Normal RBAC authorization no longer reloads five graph-wide policy scans per
tool call. A single-transaction HQL snapshot is cached by the `RbacConfig`
revision; every policy mutation advances that revision in the same write
transaction. Each authorization still reads the one config row, so grants and
revocations are visible on the next check without a TTL or a second source of
truth. The scan-free memory projections extend this to schema contract
`helixir-rbac-default-onboarding-v3`, so managed upgrades cannot silently reuse
the older non-revisioned query surface.

## Verification

- 266 library tests, 17 CLI tests, the repository-wide module-budget test, and
  the complete non-ignored test surface pass.
- Formatting, all-target/all-feature Clippy with warnings denied, rustdoc with
  warnings denied, and all-target compilation pass.
- HelixDB CLI v2.3.5 validates and compiles all 170 HQL queries.
- Live enabled-state E2E passes compatibility bootstrap, user enrollment,
  group isolation, dedup federation history, secondary actor binding, and
  preserves enabled enforcement.
- Disposable empty HelixDB instances pass both fresh-install and pre-RBAC
  legacy-upgrade bootstrap scenarios.
- Manual MCP stdio smoke registers 21 tools, 2 prompts, and 3 resources; manual
  CLI CRUD confirms enrollment, assignment, revocation history, cleanup, and
  enabled-state persistence.

## Known limitations

The Windows x86_64 release artifact includes the full NLI runtime, but Windows
does not yet have the transactional one-command PowerShell bootstrap available
on macOS and Linux. This follow-up is tracked in
[#131](https://github.com/nikita-rulenko/Helixir/issues/131). The native
installer UI remains a separate post-release project tracked in
[#105](https://github.com/nikita-rulenko/Helixir/issues/105).

## Upgrading

Back up the persistent HelixDB volume before changing the schema. With the
pinned HelixDB CLI v2.3.5, run `helix check`, rebuild the `dev` instance image,
recreate the container against the same volume, then replace the Helixir
binaries. Run `helixir doctor --json` and verify `ready: true`; finally restart
Codex, Claude Code, Cursor, gateways, and any other long-lived MCP clients so
they load the new binary and tool schemas.

RBAC activation is permanent. There is intentionally no disabled-mode or
rollback switch after migration; the `default` workspace is the supported
full-trust compatibility environment.
