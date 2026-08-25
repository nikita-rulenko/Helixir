---
name: helixir-memory
description: Use the Helixir persistent memory (MCP tools mcp__helixir-local__*) to recall and store cross-session knowledge. Invoke whenever you need to remember a fact/decision/preference across sessions, recall prior context before answering, trace WHY a past decision was made, connect two ideas, or reason through a multi-step problem in a persistent scratchpad. Use it proactively at the start of a new task (recall) and as you make decisions (capture) — not only when the user says "remember".
---

<!--
  TEMPLATE. To install as a Claude skill, copy this file to
  ~/.claude/skills/helixir-memory/SKILL.md  (rename to SKILL.md).
  Requires the `helixir-local` MCP server wired in — see integration/README.md.
  Replace `claude` with your agent's stable user_id.
-->

Helixir is a reasoning-aware memory: it stores typed facts in a knowledge graph
with causal edges, so it returns *why* things are true, not just similar text.
The read path makes zero generative/reasoning-LLM calls and is fast — search
liberally. A cold semantic query still uses the configured embedding endpoint.

Always pass a stable lower-case **`actor_id`** to every tool that accepts it and a
consistent **`user_id`** for memory ownership. They may be equal, but `user_id`
is never a credential. `claude` below is a PLACEHOLDER.

## Establish your identity (do this BEFORE the first recall)

Pick one stable principal and use it as `actor_id` on every applicable call, choosing in
this order:

1. **An id you were explicitly assigned/configured** (by the user or your host) — use it.
2. **Else derive a stable one:**
   - your **own name from your system prompt** (e.g. a prompt that says "You are
     Zeroclaw…" → `zeroclaw`), lower-kebab-case; or
   - if you run in a shell, the **OS user** (`whoami`).
3. Prefer `HELIXIR_RBAC_ACTOR` when onboarding configured it. Use the same id
   for the first `search_memory` and every later MCP/FastThink call.
4. If identity is uncertain, inspect the onboarding/client configuration or
   ask the operator. Only a global admin may call **`list_users`** under
   permanent RBAC; don't silently adopt another agent's id.

Choose one stable `user_id` for authored memory. It may equal the actor. Replace
every `claude` below with the identities you established.

For a separate agent host, run `helixir-client connect` once. The
`enroll_client` tool accepts only that host's stable `actor_id` and can grant
only `worker` in reserved `onboarding`; it is bootstrap machinery, not a normal
session tool. The client connects to the Helixir MCP gateway, never HelixDB.

## The core loop: recall → work → capture

### 1. Recall first (start of any non-trivial request — and after a summary)
```
search_memory(query="<the user's topic, in your own words>", user_id="claude", actor_id="claude")
```
If it returns `[]` for your user_id, retry once with `scope="collective"`. Read
the provenance (`origin`, `edge`, `ppr`) — graph-pulled results are related
context, not noise.

**After a context summary / compaction**, treat it as a fresh start: your first
action is to `search_memory` the topic and refresh from Helixir before
continuing. The summary is lossy; the memory is the ground truth.

### 2. Capture durable facts (proactively, as you work)
When the user states or you establish a **decision, preference, goal,
constraint, outcome, or gotcha**, store it:
```
add_memory(message="<one plain natural-language sentence>", user_id="claude", actor_id="claude", group_id="<working-group>")
```
- Pass raw prose; Helixir extracts atomic typed facts itself.
- **`needs_clarification`** → the charter refused to silently resolve a conflict.
  Ask the user the `suggested_question` (or apply a standing rule); never
  overwrite silently.
- **`ok:true`** → success, never retry. **`deduped` set with `memories_added=0`**
  (`saved>0`) → already known (success).
- Non-empty **`updated`** lists existing memory ids changed by the decision matrix.
- **`{ok:true, status:"accepted", pending_id}`** → buffered write finishing;
  success, searchable in seconds. Only **`ok:false`** is a real failure.
- **Don't store** ephemeral chatter, secrets, or facts derivable from code/git.

### 3. Capture AT the milestone, not at session end
The trigger is an event, not a schedule: a fix landed, a test went green, a
release shipped, a decision was made, a dead end was proven — `add_memory`
it IN THAT MOMENT, one plain sentence with the what and the why. Sessions
get cut off; a capture postponed to "the end" is a capture lost.

## Choosing the right retrieval tool

| You want… | Tool | Note |
|---|---|---|
| What do I know about X (default) | `search_memory` | hybrid vector+BM25+graph, PPR-ranked |
| WHY is X so / what led to it | `search_reasoning_chain` | `chain_mode="causal"` walks BECAUSE edges |
| How are A and B related | `connect_memories` | anchors = free-text **or** a `memory_id` |
| Only goals / preferences / one type | `search_by_concept` | `concept_type` enum |
| Everything for a user (audit/count) | `list_memories` | no relevance ranking |
| The graph around a memory | `get_memory_graph` | nodes + typed edges |
| Unfinished reasoning to resume | `search_incomplete_thoughts` | check when re-entering a topic |
| Outcome of a buffered write | `get_add_status` | pass `pending_id` + your `actor_id` under permanent RBAC |

To correct or annotate a stored fact, use `update_memory(memory_id, ...)` —
it amends without deleting; history is preserved.

If `search_memory` in the default `contextual` mode returns nothing on an old
corpus, retry with `mode="full"`.

## Reasoning with FastThink (multi-step analysis)

The trigger: if your next two moves would be `search_memory` and then a
judgement (comparing options, diagnosing a cause, planning against known
constraints) — open a FastThink session and do BOTH inside it. `think_recall`
puts the stored facts inside your reasoning tree; `think_commit` persists ONE
synthesized conclusion with SUPPORTS provenance edges from that evidence
(fast — seconds), so the WHY survives, not just the answer. A single plain
fact needs no session — `add_memory` it.

```
think_start(session_id="<you choose>", initial_thought="<the question>", actor_id="claude")
think_add(session_id, content="<a step>", parent_idx=<prev idx>, actor_id="claude")
think_recall(session_id, query="<known facts>", parent_idx=<idx>, user_id="claude", actor_id="claude")
think_conclude(session_id, conclusion="<the answer>", supporting_idx=[...], actor_id="claude")
think_commit(session_id, user_id="claude", actor_id="claude", group_id="<concrete group>")
```
Reuse one `session_id` and, under permanent RBAC, the same `actor_id` on every
lifecycle call. A session id is not a credential. `think_discard` is likewise
actor-bound. Historical pre-RBAC timeouts could auto-save incomplete work;
permanent RBAC timeouts fail closed because no owner/group was supplied for a
partial write.

Worked episode: "pick a retry policy" → think_start with the question →
think_add the observation ("outages last under a minute") → think_recall
("aurora outages queue") pulls two known facts in → think_conclude
("exponential backoff capped at 90s with jitter") → think_commit. Result:
one memory whose SUPPORTS edges point at the recalled evidence.

## The swarm (collective tier)

Every execution instance (root or delegated) calls
`agent_heartbeat(actor_id=<logical principal>, agent_id=<execution instance>)`
immediately on start and at meaningful progress boundaries; it never writes
fake memory for presence. Passing the same
`agent_id` on a real `add_memory` refreshes its lease. `swarm_status` groups
instances under logical principals; global-admin `list_users(actor_id=...)` =
which identities exist. In
`pending_outcomes`: `contradiction_review` → settle with
`resolve_contradiction(from_id, to_id, confirm|retract|preference)`;
`ops_alert` → the memory's health watchdog (Hygieia) — tell your human.

- **Say goodbye**: one-shot agents call `agent_farewell(actor_id=..., agent_id=...)` on
  exit — otherwise the roster shows a stale "working" forever (it will be
  flagged `derived_status: stale`, but a clean `done` is better).

## Principles
- **Recall before you re-derive** — don't make the user repeat what's stored.
- **The memory doesn't gaslight its owner** — surface `needs_clarification`,
  never silently overwrite.
- **Stable identity** — same `actor_id` for authorization and same `user_id`
  for ownership; never swap owners to bypass policy.
- **Write durable facts, not trivia.**

## Reading curated results

Search results are capped and deduplicated. `metadata.collapsed` on a result
lists same-story ids folded under it (content reachable by id — never lost).
BECAUSE edges tagged `lachesis-stitch` are retroactive hypotheses from a
background pass — present them as suspected links, not settled facts.
Generated insights carry lifecycle labels: `HYPOTHESIS (generated, ...)` =
unverified, `VERIFIED (generated, ...)` = survived witness review,
`RETIRED hypothesis` = failed review (demoted, kept for history).
`think_status.thoughts_left` shows session headroom; `think_conclude` works
even at 0.

If a recall in the conversation's language is thin, retry the query in
English — older memories may be stored in English regardless of source
language.

Explicit connectives in add_memory guarantee typed edges: "because" →
BECAUSE, "is part of" → PART_OF, "is a kind of" → IS_A (EN and RU). State
causes and structure explicitly — that is what later answers "why" without
an LLM call.

Write for the ontology too — all 8 types: "I prefer X" → preference,
"I can X" → skill, "my goal is X" → goal, "I think X" → opinion, "X is
true" → fact, "I did X" → action, "doing X, I realized Y" → experience,
"I shipped X" → achievement. Typed memories are findable memories —
`search_by_concept` and the charter's protections only work when the
type lands.

Every `resolve_contradiction` verdict teaches the charter: after several
identical verdicts the result carries a `rule_proposal` — adopt it with the
`add_memory` call it dictates; adopted rules render in `memory://rules` and
silence that question shape. A result with `superseded: true` is history —
`superseded_by` names the current version; never act on it as current truth.

To recall a period, pass `time_from`/`time_to` (RFC3339 or `YYYY-MM-DD`) to
`search_memory`. Direct answers stay inside the window (event time); linked
memories from outside return flagged `flashback: true` with their
`event_date` — present them as dated associations, not as events of that
period.

```
search_memory(query="deploys", user_id="claude", actor_id="claude",
              time_from="2026-06-01", time_to="2026-06-30")
-> June rows + {content: "...", metadata: {flashback: true,
                event_date: "2026-05-12T...", edge: "BECAUSE"}}
```
RIGHT: "Related, from May 12: …" — WRONG: presenting the May row as June.

## HelixDB v2.3.5 schema/query discipline

Helixir is pinned to its maintained **HelixDB v2.3.5 fork** in the top-level
`helixdb/` directory (the LMDB-era v2 engine). Do not run `helix update`, use
the unpatched upstream binary, use a v3/hyperscale binary, or mix v3 deployment
instructions into this repository: v3 has a different runtime and will not
register this schema. Managed-local server releases use the immutable database
image declared by the server-only `backend-image.json`; client packages never
carry or operate that runtime. The project contract is `helix.toml` at the repository root with
`queries = "helixir/schema"`, `helixir/schema/schema.hx`, and
`helixir/schema/queries.hx`.

Before touching a schema or query:

1. Read the relevant `helixir/doc/data-model.md` and `helixir/doc/architecture.md`.
2. Make additive changes where possible. Existing populated nodes must not
   receive a new non-nullable field without a migration plan; HelixDB does not
   migrate existing data for us.
3. Keep schema types exact and explicit. `id` is reserved; use domain keys such
   as `group_id` or `assignment_id`. Node and edge types must match every
   `N<>`, `Out<>`, `In<>`, `AddN<>`, and `AddE<>` use in HQL.
4. Keep queries strongly typed and start each traversal from a source step.
   Query names are API names and must match the Rust caller exactly. Use
   `AddE<Kind>::From(source)::To(target)` with both endpoints; use `UPDATE` only
   on nodes/edges, never vectors. `UpsertN` is available in the pinned v2.3.5
   toolchain and is used only with a stable domain key.

Build/check through `make build-helixdb-cli` and
`HELIX_REPO_PATH=<repo>/helixdb helixdb/target/release/helix check`. The safe
deployment sequence is mandatory:

```text
helix --version                         # must report 2.3.5
helix check                             # compile/type-check schema + queries
helix backup <instance> -o <backup-dir> # snapshot before a schema transition
# stop the instance, rebuild/recreate against the SAME persistent volume
# deploy with the repository's configured v2 flow (`helix push <instance>`)
# or the packaged `helixir-deploy` adapter when operating the self-hosted port
# verify health and call a read-only query before enabling new features
```

Never deploy a changed schema directly to a live persistent volume without a
backup. A query returning `NOT_FOUND` for a newly added RBAC query means the
backend has the old schema; it is a deployment state, not permission to fall
back to local files or silently disable authorization.

## RBAC operating contract

RBAC state is a graph in HelixDB and is the single source of truth for the CLI,
MCP server, and Rust facade. There is no local policy file to edit or cache.
RBAC is permanent. Bootstrap creates reserved `default` for pre-RBAC memories
and trusted peers, `onboarding` for newly discovered principals, and the
membership-free `moirai` workspace for global-admin-only hypotheses and their
provenance. The transition is checkpointed in HelixDB and resumes forward;
authorization is deny-by-default and fail-closed.

The graph contains `RbacConfig`, `RbacGroup`, `RbacDedupGroup`, and
`RbacAssignment` nodes plus membership, memory-visibility, and memory dedup
provenance edges. `Memory.user_id` remains the author/owner. At the API
boundary, `actor_id` is the authenticated principal whose grants are checked
and `user_id` is the target owner. Every MCP tool that exposes `actor_id` must
receive it under permanent RBAC.
FastThink lifecycle calls (`think_start/add/recall/conclude/status/discard/commit`)
must repeat that same actor; cross-principal session access is denied. Poll
`get_add_status` with `actor_id`: only the pending owner, its creator, or a
global admin may read it. Outbox payloads are owner/admin-only even when a
moderator or viewer can read the owner's group memories, because a failed
notice can contain the original raw input.
Never let a caller change `user_id` to bypass an `actor_id` check. Helixir
infers an omitted `group_id` only when exactly one reserved workspace is
writable; ambiguous membership fails closed. Working-group writes must pass one
concrete `group_id`; do not pass a `dedup_group_id` there. Only `default`
preserves legacy dedup fingerprints.
Only the bootstrap operator receives global admin; never grant every detected
agent control-plane access.

Active or historical membership in `default` or `onboarding` contributes to
the principal registry. An
administrator enrolls a new principal with `helixir rbac group add-user --group
onboarding --user <id>`, then may assign other groups. `helixir rbac user list`
projects users, active roles, assignment history, and Agent presence directly
from HelixDB. Removing a group membership deactivates the grants but retains the
User node and audit history; never maintain a second registry in local files.
For a remote client, prefer the complete server-side playbook: `helixir rbac
user onboard --user <id> --group <group> [--group-name <name>] --role <role>
--json`. It creates a missing workspace when requested, grants target access,
removes temporary onboarding membership by default, and verifies the effective
scope. Use `--keep-onboarding` only for a deliberate staged transition.

An optional dedup federation deliberately gives several groups one fingerprint
domain and common visibility. Agents always address their concrete group;
`RbacManager` resolves its current federation. Joining grants the federation's
existing history. Leaving retains already-materialized memory-to-group edges,
but future federation memories omit the departed group and its own future
writes use a private group fingerprint. Never delete historical visibility
edges or merge fingerprints across federation boundaries.

Role semantics are fixed:

- `admin`: global unrestricted read/write;
- `groupadmin`: unrestricted read/write in assigned groups;
- `moderator`: read/write in assigned groups;
- `worker`: read in assigned groups and write only memories authored by self;
- `viewer`: read-only in explicitly assigned groups.

`teamlead` is retired legacy state. Never grant it; convert existing assignments
explicitly with `helixir rbac migrate-teamleads --yes`.

Use the `helixir rbac` CLI family for management (`bootstrap`, `status`,
`group`, `dedup`, `grant`, `revoke`, `check`). Dedup management is
`dedup create|list|attach|detach|delete`; it requires a global admin. Do not infer access from a memory's
text, metadata, or the presence of a graph edge alone; resolve active
assignments through `RbacManager`. Global admin is required for management once
RBAC is enabled. The CLI principal comes from `HELIXIR_RBAC_ACTOR`; do not add
or rely on a user-supplied actor flag. If the RBAC schema is absent, report
deployment readiness and resume bootstrap after the schema is deployed; do not
treat connection or permission errors as disabled RBAC.
