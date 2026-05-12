# Userflow

> _Reflects code as of `v0.3.1-fix`. Last verified: 2026-05-12._

Helixir has exactly one user — an LLM agent — talking to it over MCP/stdio.
"Userflow" therefore means **how the agent decides which tool to call when**.

The MCP surface is fully defined at `helixir/src/mcp/server.rs:70-553`. There
are 14 tools, 2 prompts, 2 resources.

## 1. Tool catalog

### Memory tools (read/write the persistent store)

| Tool | Mandatory params | Optional params | When to call |
|---|---|---|---|
| `add_memory` | `user_id`, `message` | `agent_id` | After a user reveals a preference, makes a decision, or completes a task. |
| `search_memory` | `user_id`, `query` | `mode`, `limit`, `scope`, `temporal_days`, `graph_depth` | Session start, before reasoning, when context is needed. |
| `list_memories` | `user_id` | `limit`, `memory_type` | Audit / debugging. (Currently filters after limit — see issue #14.) |
| `update_memory` | `memory_id`, `user_id`, `new_content` | — | Correcting an existing memory's content (regenerates embedding). |
| `get_memory_graph` | `user_id` | `memory_id`, `depth` | Visualizing relationships around a node. |
| `search_by_concept` | `user_id`, `query` | `concept_type`, `tags`, `mode`, `limit` | When the agent knows it wants skills, preferences, goals, etc. |
| `search_reasoning_chain` | `user_id`, `query` | `chain_mode` (`causal`/`forward`/`both`/`deep`), `max_depth`, `limit` | Answering "why" / "what follows" questions. |
| `search_incomplete_thoughts` | — | `limit` | Session start, to resume interrupted FastThink sessions. |

### FastThink tools (ephemeral working memory)

| Tool | Mandatory params | Optional params | When to call |
|---|---|---|---|
| `think_start` | `session_id`, `initial_thought` | — | Beginning a complex reasoning task. |
| `think_add` | `session_id`, `content` | `thought_type` (`reasoning`/`hypothesis`/`observation`/`question`), `parent_idx` | Each reasoning step. |
| `think_recall` | `session_id`, `query`, `parent_idx` | `user_id` | Pulling persistent memories into the live session. |
| `think_conclude` | `session_id`, `conclusion` | `supporting_idx[]` | Marking a final answer in the session. |
| `think_commit` | `session_id`, `user_id` | — | Persisting the conclusion (runs full `add_memory` pipeline). |
| `think_discard` | `session_id` | — | Throwing away the session. Hot-path errors. |
| `think_status` | `session_id` | — | Checking remaining time / thought count. |

### Prompts and resources

| Kind | Name | Purpose |
|---|---|---|
| Prompt | `memory_summary` | Builds a "summarize all my memories about X" message for the agent. |
| Prompt | `tool_selection_guide` | The full cognitive protocol (`mcp/prompts.rs`) — when the agent should call which tool. |
| Resource | `config://helixir` | Server config snapshot. Currently misreports `version` and omits two tools (issue #14). |
| Resource | `status://helixdb` | Live HelixDB host/port. |

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

Wall-clock & thought-count limits live at `FastThinkLimits::mcp` (default
90 s, 150 thoughts). On `Timeout` during `think_add`, the manager
auto-commits the partial session — see `mcp/server.rs:322-340`.

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

## 6. Where MCP usage and code disagree (today)

- `list_memories(memory_type=X, limit=N)` may return fewer than N (or zero)
  matches because filtering happens client-side after the limit. Tracked in
  issue #14.
- `read_resource("config://helixir")` returns `version: "0.3.0"` even on
  v0.3.1+. Tracked in issue #8.
- The `read_resource("config://helixir").tools` list does not include
  `list_memories` or `search_incomplete_thoughts`. Same issue #14.

When AGENTS.md §2 ("Session boot sequence") says "read open P0 issues first",
this is one of the reasons.
