# v0.17.1 — The Governed Ledger

Released 2026-08-23.

Helixir v0.17.1 makes the physical graph contract inspectable instead of
implicit and completes the operator side of distributed-client onboarding.
Every HelixDB declaration now has a machine-checked lifecycle, while one
resumable server command can move an admitted client into its working security
domain without bypassing graph-backed RBAC.

## Physical schema ledger

- A versioned Rust inventory classifies all **22 nodes, 5 vectors and 30
  edges** as `active`, `reserved` or `deprecated` and records an owner plus
  executable evidence for every declaration.
- The current ledger contains **40 active, 16 reserved and 1 deprecated**
  declaration. `Reasoning` is explicitly deprecated in favor of the existing
  first-class reasoning edges; it is not destructively removed in this patch.
- CI parses `schema.hx` and rejects unclassified declarations, active entries
  without producer/consumer/E2E evidence, reserved entries without a milestone,
  deprecated entries without a migration plan, and drift between census keys,
  documentation and the schema.
- Three bounded read-only HQL aggregate queries count the complete physical
  contract without an unbounded client-side scan.
- The global-admin control plane exposes the same lifecycle ledger and live
  census in Hygieia's System view. Failed or unavailable queries are visible as
  diagnostics instead of silently becoming zero.

The ledger deliberately distinguishes physical storage declarations from the
eight user-facing `Memory.memory_type` values and from semantic memory
relations such as `BECAUSE`, `IMPLIES`, `SUPPORTS`, `CONTRADICTS`, `IS_A` and
`PART_OF`.

## Resumable client workspace onboarding

Server operators can now complete placement of a client that has self-enrolled
into reserved `onboarding`:

```bash
helixir rbac user onboard \
  --user codex-laptop \
  --group development \
  --group-name "Development" \
  --role worker \
  --json
```

The global-admin-only workflow:

- verifies active or historical onboarding registration before mutating state;
- creates a missing non-reserved group only when its name is supplied
  explicitly;
- grants `groupadmin`, `moderator`, `worker` or `viewer` through HelixDB;
- removes temporary onboarding access by default, with `--keep-onboarding` for
  deliberate staged placement;
- reloads policy and returns the resulting readable groups, write ability and
  security scope;
- converges safely after interruption or repeated execution and never deletes
  role history.

The thin `helixir-client` remains intentionally unable to choose a working
group or elevate itself. Helixir on the server remains the single source of
truth for placement and authorization.

## Upgrade

The physical schema declarations are unchanged, but the query bundle grows
from 182 to **185 HQL queries**. On a full host, create a recoverable cold
backup, deploy the v0.17.1 query bundle with HelixDB CLI v2.3.5, recreate the
database service against the same persistent volume, then restart the gateway
and control plane and run:

```bash
helixir doctor --json
```

Open the Schema Ledger as a global administrator and verify that all 57
declarations have a count or an explicit diagnostic. No data rewrite and no
rollback from permanent RBAC are part of this release.

Agent-only hosts update only `helixir-client`; they do not deploy HelixDB,
NLI, embeddings, Moirai, Hygieia or the control plane.
