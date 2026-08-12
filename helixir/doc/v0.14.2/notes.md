# v0.14.2 — The Governed Fates

This patch tightens the enterprise RBAC model without changing the public MCP
tool set.

## Role model

- `groupadmin` is now the multi-group team-lead permission: it can read and
  write assigned groups and manage their memberships and group-scoped roles.
- Reserved workspaces, group lifecycle, global roles, the principal registry,
  and dedup federations remain global-admin-only.
- New `teamlead` grants are rejected. Existing read-only assignments remain
  parseable until a global administrator explicitly runs
  `helixir rbac migrate-teamleads --yes`.
- Role replacement and revocation preserve the traversable membership edge
  while deactivating the matching audit grants, including historical grants
  whose assignment ids predate the deterministic id scheme.

## Moirai memory boundary

- Bootstrap now guarantees a third reserved workspace, `moirai`, with no role
  assignments. Global administrators read it through their existing bypass.
- Clotho, Lachesis, and Atropos may analyze source memories across every group,
  but only global admins can invoke them or read their generated hypotheses.
- New insights, retirement notes, and retroactive causal hypotheses use the
  salted `rbac:group:moirai` domain.
- `MOIRAI_DERIVED_FROM` records hypothesis provenance without joining ordinary
  reasoning traversal. Non-admin `connect_memories` also excludes the Clotho
  category bridge.
- Bootstrap idempotently moves historical Moirai memories out of user
  workspaces, rekeys their security domain, converts legacy `SUPPORTS`
  provenance, reifies legacy `lachesis-stitch` edges as protected hypotheses,
  removes those generated edges from the ordinary graph, and materializes the
  protected group edge. The repair filters only legacy `SUPPORTS` relations,
  preserves unrelated generic edges, and creates or restores the required
  embedding for every reified hypothesis.

## Upgrade

The schema contract is now `helixir-rbac-moirai-v4`. Use the normal
transactional onboarding/deployment flow so the HelixDB volume is backed up
before the additive schema/query deployment, then rerun RBAC bootstrap as the
global operator. The transition is resumable and does not disable RBAC.
