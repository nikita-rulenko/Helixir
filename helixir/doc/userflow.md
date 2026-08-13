# Userflow

> _Reflects code as of `v0.14.3`. Last verified: 2026-08-13._

Helixir has exactly one user — an LLM agent — talking to it over MCP/stdio.
"Userflow" therefore means **how the agent decides which tool to call when**.

The MCP surface is defined in `helixir/src/mcp/` (`server.rs` + `tools/`).
There are 21 tools, 2 prompts, and 3 resources.

## 1. Tool catalog

### Memory tools (read/write the persistent store)

| Tool | Mandatory params | Optional params | When to call |
|---|---|---|---|
| `add_memory` | `user_id`, `message` | `actor_id`, `group_id`, `agent_id` | After a user reveals a preference, makes a decision, or completes a task. Enabled non-admin writes require the concrete access `group_id`; Helixir resolves any dedup federation. Ack is confirm-or-promise (#63): `ok:true` plus `memory_ids` (new), `updated` (changed), or `deduped` (already known), or `{ok:true, status:"accepted", pending_id}` when buffered. |
| `get_add_status` | `pending_id` | `actor_id` | Polling a promised buffered write. Enabled RBAC permits only its owner, creator, or a global admin. |
| `search_memory` | `user_id`, `query` | `mode`, `limit`, `scope`, `temporal_days`, `graph_depth` | Session start, before reasoning, when context is needed. |
| `list_memories` | `user_id` | `limit`, `memory_type` | Audit / debugging. (Currently filters after limit — see issue #14.) |
| `update_memory` | `memory_id`, `user_id`, `new_content` | — | Correcting an existing memory's content (regenerates embedding). |
| `get_memory_graph` | `user_id` | `memory_id`, `depth` | Visualizing relationships around a node. |
| `search_by_concept` | `user_id`, `query` | `concept_type`, `tags`, `mode`, `limit` | When the agent knows it wants skills, preferences, goals, etc. |
| `search_reasoning_chain` | `user_id`, `query` | `chain_mode` (`causal`/`forward`/`both`/`deep`), `max_depth`, `limit` | Answering "why" / "what follows" questions. |
| `connect_memories` | `user_id`, `query_a`, `query_b` | `max_depth` | "How is A related to B?" — path between two concepts with edge types and confidence. |
| `search_incomplete_thoughts` | — | `limit` | Session start, to resume interrupted FastThink sessions. |
| `list_users` | — | `limit` | Orientation in a shared store: which identities exist. Collective-gated (`available:false` in Solo); privacy-safe (ids/names only). |
| `swarm_status` | — | `active_window_secs` | Rendezvous (#39): the live agent roster — role, host, status, seconds since last heartbeat. Collective-gated. |
| `resolve_contradiction` | `from_id`, `to_id`, `resolution` | — | Answering a `contradiction_review` notice: `confirm` / `retract` (supersedes, history kept) / `preference`. Retired disputes stop re-surfacing. |
| `agent_farewell` | `agent_id` | — | Marking a one-shot agent as done in the swarm roster without changing authorship provenance. |

Under `HELIXIR_RETRIEVAL_PROFILE=algo_opt`, `add_memory` responses may carry a
`needs_clarification` array — write-path conflicts the memory charter
(`memory-charter.md`) forbids resolving silently. Each entry has the conflict
type, the existing memory, the decision already taken and a ready-to-ask
question; the agent decides whether to ask the human.

### FastThink tools (ephemeral working memory)

| Tool | Mandatory params | Optional params | When to call |
|---|---|---|---|
| `think_start` | `session_id`, `initial_thought` | `actor_id` | Beginning a complex reasoning task; enabled RBAC binds the session to this actor. |
| `think_add` | `session_id`, `content` | `actor_id`, `thought_type` (`reasoning`/`hypothesis`/`observation`/`question`), `parent_idx` | Each reasoning step; enabled RBAC requires the bound actor. |
| `think_recall` | `session_id`, `query`, `parent_idx` | `actor_id`, `user_id` | Pulling authorized persistent memories into the live session. |
| `think_conclude` | `session_id`, `conclusion` | `actor_id`, `supporting_idx[]` | Marking a final answer in the actor-bound session. |
| `think_commit` | `session_id`, `user_id` | `actor_id`, `group_id` | Persisting the conclusion through the same RBAC-scoped write pipeline. |
| `think_discard` | `session_id` | `actor_id` | Throwing away the actor's own session. |
| `think_status` | `session_id` | `actor_id` | Checking the actor's own session status. |

When RBAC is enabled, ingest completion logging notifications are disabled:
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

## 3. Typical session shape

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

## 4. State machine: FastThink session

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
                                     │   │       │
                                think │   │ think │ auto commit_partial
                              _commit │   │_disc..│ (incomplete_thought)
                                     ▼   ▼       ▼
                              ┌──────────────────────┐
                              │  PERSISTED IN STORE  │
                              └──────────────────────┘
```

Wall-clock & thought-count limits come from `FastThinkConfig` (default
90 s, 150 thoughts). Permanent RBAC fails closed on timeout because
`think_add` does not carry the explicit owner/group needed for a scoped write;
the actor must discard and restart the timed-out session. Historical
`incomplete_thought` memories remain searchable.

## 5. Anti-patterns the agent should refuse

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

## 6. Release contract checks

`config://helixir` derives its version from `CARGO_PKG_VERSION` and enumerates
all 21 registered tools. `list_resources` exposes the three resources above.
The release smoke test must compare these advertised counts with MCP
`tools/list`, `prompts/list`, and `resources/list` after every tool-surface
change.
