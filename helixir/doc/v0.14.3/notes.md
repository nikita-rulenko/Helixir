# v0.14.3 — The Aligned Contract

This patch aligns the installed HelixDB schema contract with the v0.14.2 RBAC
runtime and makes FastThink commit acknowledgements reliable for every write
decision.

## Schema and onboarding

- The packaged `schema/queries.hx` now reports
  `helixir-rbac-moirai-v4`, matching the runtime's required contract.
- `helixir doctor` and onboarding no longer reject a freshly deployed release
  because its own packaged schema marker is stale.
- A regression test keeps the packaged HQL marker and Rust contract constant in
  lockstep.

## FastThink

- `think_commit` now returns the affected memory id when its conclusion updates
  an existing memory instead of adding or deduplicating one.
- Evidence provenance and background entity linking use the complete set of
  added, updated, and deduplicated memory ids without duplicate work.

## Agent guidance

- MCP prompts and the canonical Helixir skill now describe permanent,
  fail-closed RBAC, the `actor_id`/`group_id` write protocol, Moirai's
  global-admin boundary, asynchronous ingest outcomes, contradiction review,
  learned charter rules, and swarm presence lifecycle.
- Installer tests ensure the skill shipped to Codex, Claude Code, and other MCP
  clients retains that guidance.

## Upgrade

Use the normal transactional onboarding/deployment flow. Back up the persistent
HelixDB volume before deploying the packaged queries, keep RBAC enabled, then
run `helixir doctor`. Existing v4 data is unchanged; the deployment corrects
the reported schema contract and is safe to repeat.
