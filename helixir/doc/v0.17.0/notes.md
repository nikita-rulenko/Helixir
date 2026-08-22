# v0.17.0 — The Distributed Family

Released 2026-08-22.

Helixir v0.17.0 makes the memory service genuinely distributable. A full host
continues to own HelixDB, models, governance and operations, while agent-only
machines can install a small client and connect to that host through the MCP
gateway. The same release also stops treating every short-lived execution as a
separate human-facing agent.

## Independent remote client

- `helixir-client` is a sibling Rust package for agent-only hosts. It contains
  no HelixDB runtime, models, watchdog, control plane or admin credentials.
- Homebrew and APT publish `helixir-client` separately from the full `helixir`
  package.
- `helixir-client connect` registers Codex, Claude Code and Cursor against one
  existing streamable-HTTP gateway and installs the canonical Helixir skill
  plus managed `AGENTS.md` guidance.
- `helixir-client doctor` verifies gateway reachability, MCP negotiation,
  the complete 23-tool capability surface, compatible server major/minor
  version, bounded RBAC admission, local registrations and instruction freshness. The
  release gate separately proves scoped read/write and rejects the HelixDB port
  as an MCP endpoint.
- Cursor registration is written atomically, preserves unrelated MCP entries,
  refuses symlink targets and remains private (`0600`) on Unix hosts.

## Bounded enrollment

- A new principal may enroll itself only as a `worker` in the reserved
  `onboarding` workspace.
- Repeated enrollment is idempotent and never restores a removed membership or
  upgrades a role.
- The authenticated `actor_id` remains the RBAC identity. Memory `user_id` is
  provenance and cannot be used to select another security family.

## Logical agents and transient subagents

- `Agent.principal_id` explicitly binds every execution instance to its stable
  logical principal.
- The new idempotent `agent_heartbeat` MCP tool records presence without
  forcing an agent to write a fake memory.
- Presence is explicit for both root agents and delegated executions. Ordinary
  reads, initialization, enrollment and status inspection never create or
  refresh a lease.
- `swarm_status` and the admin control plane now report logical families,
  active logical agents, total/active execution instances and child subagents
  separately.
- A logical family is online while its root or any child lease is active.
  Farewell terminates only the named instance, so concurrent siblings remain
  visible and online.
- Heartbeat ownership is fail-closed: an authenticated principal cannot claim,
  refresh, terminate or write through an execution instance owned by another
  family. Unknown farewell calls are idempotent and do not create Agent rows.
- Durable memory authorship prevents administrative pruning of an execution
  row even after its live lease has ended.
- Legacy Agent rows with an absent or null `principal_id` remain readable.
  Conservative exact-id and longest-prefix grouping is presentation fallback
  only; new rows always persist the authenticated principal explicitly.

## Release-quality client proof

The release gate now covers:

- fresh Debian/Ubuntu APT installation of the independent client;
- two concurrent client containers with distinct principals;
- explicit root and child heartbeat/farewell plus multiple execution instances
  under one principal;
- onboarding-only enrollment, group visibility and dedup-scope isolation;
- direct-network gateway read/write and explicit rejection of the HelixDB
  database port as an MCP endpoint;
- native Homebrew formula preflight on both Apple Silicon and Intel macOS
  release runners.

A production-shaped remote-host smoke also preserved 4,195 existing memories
while proving enrollment, recall and write through one gateway with no legacy
per-session stdio servers.

## Upgrade

The full host has an additive schema/query transition. Back up the persistent
volume, deploy the v0.17.0 schema with HelixDB CLI v2.3.5, restart the database
on the same volume, restart the single gateway, then run:

```bash
helixir doctor --json
```

Agent-only hosts do not deploy a schema:

```bash
brew install nikita-rulenko/tap/helixir-client
# or: sudo apt install helixir-client
helixir-client connect --gateway helixir-host:8765 \
  --principal codex-laptop --owner codex --project "$PWD"
helixir-client doctor
```

The broader reconciliation of declared schema with every live graph semantic
remains tracked separately in issue #157; this release does not claim that
work complete.
