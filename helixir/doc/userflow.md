# Userflow

> _Reflects code as of `v0.17.1`. Last verified: 2026-08-23._

Helixir has two deliberate interaction surfaces: LLM agents use MCP/stdio (or
the authenticated gateway), while a human global administrator uses the CLI or
web control plane. Sections 1–7 describe **how an agent decides which tool to
call and reads its result**; section 8 describes the separate administrative
flow.

The MCP surface is defined in `helixir/src/mcp/` (`server.rs` + `tools/`).
There are 23 tools, 2 prompts, and 3 resources.

## 1. Tool catalog

### Memory tools (read/write the persistent store)

| Tool | Mandatory params | Optional params | When to call |
|---|---|---|---|
| `add_memory` | `actor_id`, `user_id`, `message` | `group_id`, `agent_id` | After a user reveals a preference, makes a decision, or completes a task. Non-admin working-group writes require the concrete access `group_id`; Helixir resolves any dedup federation. Ack is confirm-or-promise (#63): `ok:true` plus `memory_ids` (new), `updated` (changed), or `deduped` (already known), or `{ok:true, status:"accepted", pending_id}` when buffered. |
| `get_add_status` | `actor_id`, `pending_id` | — | Polling a promised buffered write. RBAC permits only its owner, creator, or a global admin. |
| `search_memory` | `actor_id`, `user_id`, `query` | `mode`, `limit`, `scope`, `temporal_days`, `time_from`, `time_to`, `graph_depth` | Session start, before reasoning, when context is needed. Scope never widens RBAC visibility. |
| `list_memories` | `actor_id`, `user_id` | `limit`, `memory_type` | Bounded newest-first audit/debug view; filtering is applied by the query contract. |
| `update_memory` | `actor_id`, `memory_id`, `user_id`, `new_content` | — | Correcting an existing memory's content (regenerates embedding). |
| `get_memory_graph` | `actor_id`, `user_id` | `memory_id`, `depth` | Visualizing authorized relationships around a node. |
| `search_by_concept` | `actor_id`, `user_id`, `query` | `concept_type`, `tags`, `mode`, `limit` | When the agent knows it wants skills, preferences, goals, etc. |
| `search_reasoning_chain` | `actor_id`, `user_id`, `query` | `chain_mode` (`causal`/`forward`/`both`/`deep`), `max_depth`, `limit` | Answering "why" / "what follows" questions. |
| `connect_memories` | `actor_id`, `user_id`, `query_a`, `query_b` | `max_depth` | "How is A related to B?" — authorized path between two concepts with edge types and confidence. |
| `search_incomplete_thoughts` | `actor_id` | `user_id`, `limit` | Locate historical pre-RBAC incomplete FastThink memories; current RBAC timeouts do not auto-persist. |
| `list_users` | `actor_id` | `limit` | Global-admin registry orientation: which identities exist. Also collective-tier gated; returns only ids/names/timestamps. |
| `enroll_client` | `actor_id` | — | One narrow trusted-network admission call for a remote client. It can create only this principal as `worker` in reserved `onboarding`; existing or historical admission is returned without changing later admin-assigned roles. |
| `agent_heartbeat` | `actor_id`, `agent_id` | `status` | Publish or refresh a concrete root or delegated execution instance without writing memory. The instance is grouped under the resolved logical principal; status must be non-terminal and bounded. Call on start and meaningful progress boundaries. |
| `swarm_status` | — | `active_window_secs` | Rendezvous (#39): logical `families`, child `subagents`, and the complete diagnostic instance roster. `active`/`total` count logical principals; instance/subagent counters expose concurrency. Collective-gated. |
| `resolve_contradiction` | `from_id`, `to_id`, `resolution` | — | Answering a `contradiction_review` notice: `confirm` / `retract` (supersedes, history kept) / `preference`. Retired disputes stop re-surfacing. |
| `agent_farewell` | `actor_id`, `agent_id` | — | Marking one owned execution instance as done without changing authorship provenance; cross-principal termination is rejected. |

Under `HELIXIR_RETRIEVAL_PROFILE=algo_opt`, `add_memory` responses may carry a
`needs_clarification` array — write-path conflicts the memory charter
(`memory-charter.md`) forbids resolving silently. Each entry has the conflict
type, the existing memory, the decision already taken and a ready-to-ask
question; the agent decides whether to ask the human.

### FastThink tools (ephemeral working memory)

| Tool | Mandatory params | Optional params | When to call |
|---|---|---|---|
| `think_start` | `actor_id`, `session_id`, `initial_thought` | — | Beginning a complex reasoning task and binding it to this actor. |
| `think_add` | `actor_id`, `session_id`, `content` | `thought_type` (`reasoning`/`hypothesis`/`observation`/`question`), `parent_idx` | Each reasoning step under the bound actor. |
| `think_recall` | `actor_id`, `session_id`, `query`, `parent_idx` | `user_id` | Pulling authorized persistent memories into the live session. |
| `think_conclude` | `actor_id`, `session_id`, `conclusion` | `supporting_idx[]` | Marking a final answer in the actor-bound session. |
| `think_commit` | `actor_id`, `session_id`, `user_id` | `group_id` | Persisting the conclusion through the same RBAC-scoped write pipeline. |
| `think_discard` | `actor_id`, `session_id` | — | Throwing away the actor's own session. |
| `think_status` | `actor_id`, `session_id` | — | Checking the actor's own session status. |

`actor_id` remains `Option` in applicable wire structs only for the internal
pre-bootstrap compatibility path. In normal permanent-RBAC operation it is
mandatory whenever the tool schema exposes it. Five trusted-endpoint support
tools have narrower identity contracts: `enroll_client` takes only its own
principal and exposes no target role/group, `agent_heartbeat` binds its concrete
`agent_id` to the resolved `actor_id`, `swarm_status` takes only its time window,
`agent_farewell` takes the owning `actor_id` plus exact `agent_id`, and
`resolve_contradiction` takes the exact ids from a delivered notice.

Under permanent RBAC, ingest completion logging notifications are disabled:
they carry no request actor. Poll with `get_add_status`, or receive the result
through the authorized opportunistic outbox on a later `add_memory` call.

### Prompts and resources

| Kind | Name | Purpose |
|---|---|---|
| Prompt | `memory_summary` | Builds a "summarize all my memories about X" message for the agent. |
| Prompt | `tool_selection_guide` | The full cognitive protocol (`mcp/prompts.rs`) — when the agent should call which tool. |
| Resource | `config://helixir` | Server version, backend, capability, and complete tool snapshot. |
| Resource | `status://helixdb` | Live HelixDB host/port. |
| Resource | `memory://rules` | Human charter plus adopted learned rules. |

## 2. Tool selection — by intent

```
agent intent                                tool to call
─────────────────────────────────────────────────────────────────────
"What does the user usually prefer?"        search_by_concept(preference)
"Why did we choose X last week?"            search_reasoning_chain(causal)
"How are A and B connected?"                connect_memories(A, B)
"What happened during period X?"            search_memory(time_from/time_to)
"What's true about the user as of today?"   search_memory(mode=contextual)
"Resume yesterday's research"               search_incomplete_thoughts
                                            → think_start with recalled
                                              thoughts as initial_thought
"Show me everything"                        list_memories  (debug only)
"User just decided X"                       add_memory
"User reversed an earlier opinion"          add_memory  (decision engine
                                            will pick SUPERSEDE)
"Think this through step by step"           think_start → think_add×N →
                                            (think_recall to enrich) →
                                            think_conclude → think_commit
"What were my supporting facts for Y?"      get_memory_graph + chain
"Other users' shared knowledge on Z"        search_memory(scope=collective)
```

## 3. Reading curated results

Search responses are not raw nearest-neighbour dumps. Ranking, RBAC filtering,
graph expansion, family collapse and historical annotation have already run.
Read the metadata before presenting a row as current truth.

| Metadata | Meaning | Agent rule |
|:---------|:--------|:-----------|
| `origin`, `edge`, `parent`, `ppr`, `cosine` | Why the row entered the result and how it ranked | Preserve provenance when a conclusion depends on the graph path. |
| `collapsed: [ids]` | Same-story raw/atomic family members folded under this representative | Do not claim the content was deleted; fetch a folded id only when exact wording matters. |
| `superseded: true`, `superseded_by` | Reachable historical state replaced by a newer memory | Never act on it as current truth; follow the successor. |
| `flashback: true`, `event_date` | Graph-linked context from outside an explicit time window | Present it separately and dated, never as an event inside the requested period. |
| `collapsed_holders`, controversy metadata | Scoped Hive consensus or disagreement among authorized owners | Describe consensus only inside the actor's visible RBAC domain. |

### Event-time windows and flashbacks

Use `time_from` and/or `time_to` for a named period. Values accept RFC3339 or
`YYYY-MM-DD`; bare dates expand to the inclusive start/end of that day. An
explicit bound overrides `temporal_days`, either side may be open, and a lower
bound after the upper bound is rejected.

```text
search_memory(
  actor_id="codex",
  user_id="Codex",
  query="rollout failures",
  time_from="2026-06-01",
  time_to="2026-06-30"
)
```

The window constrains direct seed attention by event time (`valid_from` when
present, otherwise `created_at`). Authorized graph expansion remains free to
bring back older or newer context. Those rows carry
`metadata.flashback=true` plus their true `event_date` and use a separate
allowance (`retrieval.flashback_max`, default 3), so they do not displace the
requested period's rows.

Correct presentation:

```text
During June: <in-window findings>.
Related, from 2026-05-12: <flashback context>.
```

Without an explicit window, `mode=full` removes the mode-derived temporal
cutoff but never an RBAC bound. Explicit windows also never widen group
visibility.

### Write acknowledgements and delayed outcomes

Every `add_memory` result must be interpreted by contract:

- `ok:true` is success and must not be retried;
- `memory_ids`, `updated`, or non-empty `deduped` describe a completed outcome;
- `status="accepted"` plus `pending_id` is a promised buffered outcome, not a
  failure; poll `get_add_status` only when immediate confirmation matters;
- `needs_clarification` means the charter requires the human question supplied
  by the response;
- `pending_outcomes` delivers earlier authorized notices. Surface `ops_alert`
  to the operator and settle `contradiction_review` with
  `resolve_contradiction(confirm|retract|preference)` rather than guessing.

Repeated identical contradiction verdicts may produce a `rule_proposal`. Adopt
only the exact proposed `add_memory` call (or ask the operator); active charter
and learned rules are readable through `memory://rules`.

## 4. Typical session shape

```
┌─────────────────────────────────────────────────────────────┐
│  SESSION START                                              │
│                                                             │
│   1. search_incomplete_thoughts(limit=3)                    │
│        → resume any timed-out FastThink session             │
│                                                             │
│   2. search_memory(query=task_description, mode=recent)     │
│        → pull recent context                                │
│                                                             │
│   3. If insufficient:                                       │
│        search_memory(mode=deep)                             │
│        search_by_concept for typed lookups                  │
│        search_reasoning_chain for "why" questions           │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│  WORK                                                       │
│                                                             │
│   For complex decisions:                                    │
│     think_start("…")                                        │
│     think_add(reasoning), think_add(hypothesis), ...        │
│     think_recall(query, parent_idx)  ── pull facts in       │
│     think_status            ── check budget                 │
│     think_conclude(answer, supporting_idx=[...])            │
│     → think_commit  OR  think_discard                       │
│                                                             │
│   For straightforward observations:                         │
│     add_memory(message="…")                                 │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│  END                                                        │
│                                                             │
│   add_memory for any new decision / outcome that wasn't     │
│   committed via FastThink.                                  │
│   (Agent should NOT save: grep output, tool dumps,          │
│    transient state.)                                        │
└─────────────────────────────────────────────────────────────┘
```

## 5. State machine: FastThink session

```
                  think_start
   ─────────────────────────────────►   ┌────────────┐
                                        │  THINKING  │
                                        └──┬─────┬───┘
                  think_add (loop)         │     │
   ◄─────────────────────────────────────  │     │
                  think_recall (loop)      │     │
   ◄─────────────────────────────────────  │     │
                  think_status (read)      │     │
   ◄─────────────────────────────────────  │     │
                                           │     │
                                  ┌────────▼─┐ ┌─▼─────────┐
                  think_conclude  │ DECIDED  │ │  TIMED-OUT│
                                  └──┬───┬───┘ └─┬─────────┘
                                     │   │       │ discard/restart
                                think │   │ think │ (no implicit write)
                              _commit │   │_discard
                                     ▼   ▼
                              ┌──────────────────────┐
                              │ PERSISTED / DISCARDED│
                              └──────────────────────┘
```

Wall-clock & thought-count limits come from `FastThinkConfig` (default
90 s, 150 thoughts). Permanent RBAC fails closed on timeout because
`think_add` does not carry the explicit owner/group needed for a scoped write;
the actor must discard and restart the timed-out session. Historical
`incomplete_thought` memories remain searchable.

## 6. Anti-patterns the agent should refuse

The cognitive protocol prompt (`mcp/prompts.rs`) encodes these. Mirroring
them here so they live in the engineering doc too:

- **Don't dump search results into memory.** `add_memory` is for facts, not
  for tool output.
- **Don't call `search_memory` with `mode=full` as the default.** Use
  `recent` or `contextual`. Only use `full` when explicitly justified.
- **Don't bypass FastThink for complex reasoning.** It exists specifically
  to keep intermediate thoughts out of long-term memory until committed.
- **Don't call `update_memory` to "rephrase" a memory.** Persisting a new
  Memory will trigger UPDATE / SUPERSEDE through the decision engine — let
  the engine decide.

## 7. Release contract checks

`config://helixir` derives its version from `CARGO_PKG_VERSION` and enumerates
all 23 registered tools. `list_resources` exposes the three resources above.
The release smoke test must compare these advertised counts with MCP
`tools/list`, `prompts/list`, and `resources/list` after every tool-surface
change.

## 8. Global-admin control flow

The browser UI is an administration surface, not another agent role. Every API
route after bootstrap requires the graph-backed global `admin` role; a
`groupadmin`, moderator, worker or viewer receives `403` before counts or names
are projected. The browser never owns a second principal, group or ACL registry.

The Stewardship page is the post-install control room. It reads a redacted
effective configuration, accepts only an allowlisted partial patch, shows the
exact diff before apply, and treats provider credentials as write-only
replacements. The managed backup vault accepts archive ids rather than paths.
Restore requires the exact phrase `RESTORE <backup-id>`, creates a cold safety
snapshot first, and rejects or rolls back a database that does not expose the
current schema contract. Existing-local and remote databases remain observable,
but Helixir refuses to take over their backup lifecycle.
