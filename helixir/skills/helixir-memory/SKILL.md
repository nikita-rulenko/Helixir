---
name: helixir-memory
description: Use Helixir persistent graph memory through its MCP tools. Invoke at the start of non-trivial work, after context compaction, when recalling past decisions, when storing durable outcomes, when tracing why facts are connected, and when using FastThink or Helixir RBAC.
---

# Helixir memory

Treat Helixir as persistent, reasoning-aware memory shared by agents. Recall
before re-deriving, and capture durable decisions at the moment they are made.

## Establish identity

Choose one stable identity before the first call:

1. Use the principal configured by the host, normally
   `HELIXIR_RBAC_ACTOR` (`claude`, `codex`, or `cursor` after onboarding).
2. Otherwise use an explicitly assigned agent name, then the OS user as a
   last resort.
3. Use the same lower-case value as `actor_id` on every tool that accepts it.
4. Use one stable `user_id` for memory ownership. It may equal `actor_id`, but
   it is not an authorization credential.
5. If identity is uncertain, read the onboarding/client configuration or ask
   the operator. Only a global admin may use `list_users`; never silently adopt
   another principal.

## Recall, work, capture

At the start of every non-trivial request, and immediately after a summary or
context compaction, call:

```text
search_memory(query="<current topic>", user_id="<stable owner>", actor_id="<principal>")
```

If personal recall is empty, retry once with `scope="collective"`. Use
`mode="full"` when an expected older fact is absent.

Store decisions, constraints, preferences, goals, outcomes, and hard-won
gotchas with `add_memory`. Do not store secrets, ephemeral chatter, or facts
trivially derivable from code or git.

Interpret write results exactly:

- `ok:true` is success and must not be retried.
- Non-empty `updated` contains ids of existing memories changed by the decision matrix.
- `memories_added:0` plus non-empty `deduped` means already known.
- `status:"accepted"` plus `pending_id` means buffered success; poll with
  `get_add_status` only when the outcome is needed immediately.
- `needs_clarification` means the charter refused a silent conflict. Ask the
  suggested question or apply an established standing rule.
- Only `ok:false` or `status:"failed"` is failure.

Presence is explicit: transport initialization and ordinary reads never create
or refresh a lease. A root agent, worker, or sub-agent calls `agent_heartbeat`
immediately on start and at meaningful progress boundaries, using its stable
logical `actor_id` plus a concrete `agent_id`; writes carrying that `agent_id`
refresh the same lease. One-shot agents call
`agent_farewell(actor_id, agent_id)` when done; that terminal status remains
inactive until another explicit heartbeat or attributed write, while the
heartbeat window is the crash and idle fallback.

## Select the right tool

| Need | Tool |
|---|---|
| General semantic recall | `search_memory` |
| Why a decision exists | `search_reasoning_chain` with `chain_mode="causal"` |
| Relationship between two anchors | `connect_memories` |
| One ontology type | `search_by_concept` |
| Bulk audit or count | `list_memories` |
| Graph around one memory | `get_memory_graph` |
| Correct one known row | `update_memory` |
| Find historical pre-RBAC partial reasoning | `search_incomplete_thoughts` |
| Check an accepted async write | `get_add_status` with its `pending_id` |
| Announce a sub-agent without writing memory | `agent_heartbeat` |
| See active agents | `swarm_status` |
| Administer/orient registered identities | `list_users` with admin `actor_id` |
| Bootstrap this remote client once | `enroll_client` with its own `actor_id` |
| Settle a charter dispute | `resolve_contradiction` |

Results are curated. `metadata.collapsed` lists folded same-story ids.
`superseded:true` is historical; follow `superseded_by`.
Lachesis-generated causal proposals belong to the global-admin-only `moirai`
workspace and are not ordinary `BECAUSE` facts. For time windows, present
`flashback:true` rows with their original
`event_date`, not as events inside the requested period.

## Use FastThink for multi-step judgement

When a task needs recalled evidence plus a decision, use:

```text
think_start(session_id, initial_thought, actor_id)
think_add(session_id, content, parent_idx, actor_id)
think_recall(session_id, query, parent_idx, user_id, actor_id)
think_conclude(session_id, conclusion, supporting_idx, actor_id)
think_commit(session_id, user_id, actor_id, group_id?)
```

Repeat the same actor throughout the lifecycle. A session id is not a
credential. Commit one coherent conclusion; discard dead ends. Under permanent
RBAC, a timed-out session fails closed rather than guessing an owner/group and
auto-persisting a partial conclusion.

## RBAC operating contract

RBAC is permanently enabled, graph-backed in HelixDB, and the single source of
truth for the Rust facade, MCP, and CLI. Bootstrap creates three reserved groups:

- one operator receives global `admin`;
- `default` receives all pre-RBAC memories and principals as equal
  `groupadmin` peers, preserving legacy full-trust visibility and fingerprints;
- `onboarding` admits newly discovered principals as `worker` before an admin
  assigns normal working groups;
- membership-free `moirai` stores generated hypotheses and provenance for
  global administrators only;
- omitted `group_id` is inferred only when exactly one reserved workspace is
  writable; ambiguous membership fails closed;
- the transition is one-way, checkpointed, idempotent, and resumable. Never
  disable RBAC to recover from an interrupted bootstrap.

`default` recreates the historical shared data plane, not the control plane.
Never grant every agent global `admin`; full-trust compatibility comes from
equal group-admin membership in `default`.

Active or historical membership in either reserved group is part of the
graph-backed principal registry. New principals must enter through `onboarding`
before an admin assigns other groups. Removing a principal from a group
deactivates its grants but keeps the User node and assignment history; never
maintain a second local user list.

On an agent-only host, `helixir-client connect` performs the one-time
`enroll_client` call. That tool accepts only the caller's stable `actor_id` and
can grant only `worker` in reserved `onboarding`; it exposes no role or group
choice. Do not call it as a normal memory operation, do not use it to restore a
revoked role, and never point the client at the HelixDB port—the endpoint is the
Helixir MCP gateway. Administrators move admitted principals with `helixir rbac`.
The canonical server-side workflow is `helixir rbac user onboard --user <id>
--group <group> [--group-name <name>] --role <role> --json`: it creates a
missing workspace when explicitly named, grants the working role, removes the
temporary onboarding grant by default, and verifies the resulting scope.

When writing to any working group, pass its concrete `group_id` on
`add_memory` and `think_commit`. Never pass a dedup federation id. An omitted
group is accepted only when Helixir can infer exactly one reserved workspace.
Authorization is deny-by-default and fail-closed.

Roles:

- `admin`: global read/write and RBAC management;
- `groupadmin`: read/write and membership/role management in one or more
  assigned non-reserved groups;
- `moderator`: read/write assigned groups;
- `worker`: read group memories and write only their own authorship;
- `viewer`: read-only assigned groups.

`teamlead` is a retired legacy grant. Never assign it; a global administrator
may explicitly convert old assignments with `helixir rbac migrate-teamleads --yes`.
The Moirai may analyze every working group, but only a global `admin` can invoke
them or read their `moirai` workspace. `MOIRAI_DERIVED_FROM` provenance is not
part of ordinary reasoning traversal. The admin-only web control plane may
project those witness edges for inspection; this never makes them recallable by
non-admin agents. A Moirai hypothesis with zero witness edges is an integrity
failure, not an ungrounded fact to consume.

Pending results are visible only to their owner, creator, or global admin.
Outbox payloads are owner/admin-only. Never change `user_id` to bypass an
`actor_id` check.

Every `add_memory` result may also deliver `pending_outcomes` from earlier
buffered work:

- `contradiction_review` must be settled with `resolve_contradiction` using
  `confirm`, `retract`, or `preference`; all three preserve history;
- `ops_alert` is a Hygieia health incident or recovery and must be surfaced to
  the operator;
- repeated contradiction verdicts may return a `rule_proposal`. Adopt only the
  exact proposed `add_memory` call (or ask the operator); active rules are
  readable through `memory://rules`.

Use `swarm_status` before parallel work or unexplained load. The MCP session
does not infer presence from transport initialization or ordinary reads. A
root or distinct worker calls `agent_heartbeat(actor_id, agent_id, status)` at
start and progress boundaries;
passing that `agent_id` on a real write refreshes the lease but fake memory is
never required. Call `agent_farewell(actor_id, agent_id)` when a one-shot agent exits.
`agent_farewell` is authoritative immediately; never keep treating a terminal
`done`, `failed`, `offline`, `stopped`, `disconnected`, or `farewell` status as
online merely because its timestamp is fresh.

Manage policy only through `helixir rbac`. Useful commands:

```text
helixir rbac bootstrap --operator <id> --principal codex --principal claude
helixir rbac status --json
helixir rbac user list --json
helixir rbac user onboard --user <id> --group <group> --group-name <name> --role <role> --json
helixir rbac group create --id <id> --name <name>
helixir rbac group add-user --group onboarding --user <id> --role worker
helixir rbac group add-user --group <group> --user <id> --role <role>
helixir rbac group remove-user --group <group> --user <id>
helixir rbac dedup attach --group <group> --dedup-group <federation>
```

Dedup federation membership is resolved server-side. Joining grants access to
existing federation history. Leaving preserves historical visibility but
isolates future writes. Do not cache or reproduce this mapping client-side.

RBAC without transport authentication remains trusted-network role separation,
not protection against a malicious client that can spoof `actor_id`.

## HelixDB schema discipline

This repository targets Helix CLI v2.3.5. Never run `helix update` or use the
v3 hyperscale engine. HQL supports `//` line comments, not block comments.

Before changing schema or queries:

1. Read `helixir/doc/data-model.md` and `helixir/doc/architecture.md`.
2. Prefer additive changes and avoid new non-nullable fields on populated
   nodes without a migration.
3. Keep node, edge, direction, and query names exact.
4. Run `helix check`.
5. Before a live transition, back up the persistent volume, stop writers,
   deploy against the same volume, and perform read-only verification.

A missing RBAC query means the deployed schema is stale. Surface the error;
never fall back to a local ACL or silently disable enforcement.
