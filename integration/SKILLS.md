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

Always pass a **consistent `user_id`** so memory stays coherent and personal
search is scoped to you. `claude` below is a PLACEHOLDER — see "Establish your
identity" first and use YOUR id everywhere.

## Establish your identity (do this BEFORE the first recall)

The `user_id` is YOUR name in the memory — using the wrong one recalls someone
else's memories as if they were yours. Pick ONE stable id and use it on every
call, choosing in this order:

1. **An id you were explicitly assigned/configured** (by the user or your host) — use it.
2. **Else derive a stable one:**
   - your **own name from your system prompt** (e.g. a prompt that says "You are
     Zeroclaw…" → `zeroclaw`), lower-kebab-case; or
   - if you run in a shell, the **OS user** (`whoami`).
3. Use that SAME id for the first `search_memory` AND every `add_memory`.
4. If recall under it is empty/thin and you're unsure, call **`list_users`** to
   see who already exists, then confirm or correct your id — don't silently adopt
   another agent's id.

Replace every `claude` below with the id you established.

## The core loop: recall → work → capture

### 1. Recall first (start of any non-trivial request — and after a summary)
```
search_memory(query="<the user's topic, in your own words>", user_id="claude")
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
add_memory(message="<one plain natural-language sentence>", user_id="claude")
```
- Pass raw prose; Helixir extracts atomic typed facts itself.
- **`needs_clarification`** → the charter refused to silently resolve a conflict.
  Ask the user the `suggested_question` (or apply a standing rule); never
  overwrite silently.
- **`ok:true`** → success, never retry. **`deduped` set with `memories_added=0`**
  (`saved>0`) → already known (success).
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
think_start(session_id="<you choose>", initial_thought="<the question>")
think_add(session_id, content="<a step>", parent_idx=<prev idx>)   # repeat
think_recall(session_id, query="<known facts>", parent_idx=<idx>)  # evidence in
think_conclude(session_id, conclusion="<the answer>", supporting_idx=[...])  # REQUIRED before commit
think_commit(session_id, user_id="claude")   # persists once, in seconds
```
Reuse one `session_id`. `think_discard(session_id)` throws it away unsaved.

Worked episode: "pick a retry policy" → think_start with the question →
think_add the observation ("outages last under a minute") → think_recall
("aurora outages queue") pulls two known facts in → think_conclude
("exponential backoff capped at 90s with jitter") → think_commit. Result:
one memory whose SUPPORTS edges point at the recalled evidence.

## The swarm (collective tier)

Pass your stable `agent_id` on every `add_memory` — it heartbeats your
presence into the shared roster for free. `swarm_status` = who else is
here right now; `list_users` = which identities exist. In
`pending_outcomes`: `contradiction_review` → settle with
`resolve_contradiction(from_id, to_id, confirm|retract|preference)`;
`ops_alert` → the memory's health watchdog (Hygieia) — tell your human.

## Principles
- **Recall before you re-derive** — don't make the user repeat what's stored.
- **The memory doesn't gaslight its owner** — surface `needs_clarification`,
  never silently overwrite.
- **One identity** — same `user_id` everywhere.
- **Write durable facts, not trivia.**

## Reading curated results

Search results are capped and deduplicated. `metadata.collapsed` on a result
lists same-story ids folded under it (content reachable by id — never lost).
BECAUSE edges tagged `lachesis-stitch` are retroactive hypotheses from a
background pass — present them as suspected links, not settled facts.
`think_status.thoughts_left` shows session headroom; `think_conclude` works
even at 0.

If a recall in the conversation's language is thin, retry the query in
English — older memories may be stored in English regardless of source
language.

Explicit connectives in add_memory guarantee typed edges: "because" →
BECAUSE, "is part of" → PART_OF, "is a kind of" → IS_A (EN and RU). State
causes and structure explicitly — that is what later answers "why" without
an LLM call.

To recall a period, pass `time_from`/`time_to` (RFC3339 or `YYYY-MM-DD`) to
`search_memory`. Direct answers stay inside the window (event time); linked
memories from outside return flagged `flashback: true` with their
`event_date` — present them as dated associations, not as events of that
period.
