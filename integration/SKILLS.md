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
The read path makes zero LLM calls and is fast — search liberally.

Always pass a **consistent `user_id`** (e.g. `claude`) so memory stays coherent
and personal search is scoped to you.

## The core loop: recall → work → capture

### 1. Recall first (start of any non-trivial request)
```
search_memory(query="<the user's topic, in your own words>", user_id="claude")
```
If it returns `[]` for your user_id, retry once with `scope="collective"`. Read
the provenance (`origin`, `edge`, `ppr`) — graph-pulled results are related
context, not noise.

### 2. Capture durable facts (proactively, as you work)
When the user states or you establish a **decision, preference, goal,
constraint, outcome, or gotcha**, store it:
```
add_memory(message="<one plain natural-language sentence>", user_id="claude")
```
- Pass raw prose; Helixir extracts atomic typed facts itself.
- **`needs_clarification`** → the charter refused to silently resolve a conflict.
  Ask the user the `suggested_question` (or apply a standing rule); never
  overwrite silently.
- **`deduped` set, `memories_added=0`** → already known (success).
- **`{pending_id, queued}`** → async write; `get_add_status(pending_id)` to confirm.
- **Don't store** ephemeral chatter, secrets, or facts derivable from code/git.

### 3. Capture at the end of each meaningful step
After a fix, decision, or milestone, record the outcome so the next session
inherits it.

## Choosing the right retrieval tool

| You want… | Tool | Note |
|---|---|---|
| What do I know about X (default) | `search_memory` | hybrid vector+BM25+graph, PPR-ranked |
| WHY is X so / what led to it | `search_reasoning_chain` | `chain_mode="causal"` walks BECAUSE edges |
| How are A and B related | `connect_memories` | anchors = free-text **or** a `memory_id` |
| Only goals / preferences / one type | `search_by_concept` | `concept_type` enum |
| Everything for a user (audit/count) | `list_memories` | no relevance ranking |
| The graph around a memory | `get_memory_graph` | nodes + typed edges |

If `search_memory` in the default `contextual` mode returns nothing on an old
corpus, retry with `mode="full"`.

## Reasoning with FastThink (multi-step analysis)

Use the scratchpad instead of spamming `add_memory` with half-thoughts. Order:
```
think_start(session_id="<you choose>", initial_thought="<the question>")
think_add(session_id, content="<a step>", parent_idx=<prev idx>)   # repeat
think_recall(session_id, query="<known facts>", parent_idx=<idx>)  # optional
think_conclude(session_id, conclusion="<the answer>", supporting_idx=[...])  # REQUIRED before commit
think_commit(session_id, user_id="claude")   # persists; HEAVY (tens of s) — call once
```
Reuse one `session_id`. `think_discard(session_id)` throws it away unsaved.

## Principles
- **Recall before you re-derive** — don't make the user repeat what's stored.
- **The memory doesn't gaslight its owner** — surface `needs_clarification`,
  never silently overwrite.
- **One identity** — same `user_id` everywhere.
- **Write durable facts, not trivia.**
