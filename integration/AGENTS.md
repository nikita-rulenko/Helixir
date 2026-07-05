# AGENTS.md — Helixir memory

> Template. Copy to your project root as `AGENTS.md` (or merge this section into
> your existing one). Replace `claude` with your agent's stable `user_id`.
> Requires the `helixir-local` MCP server wired into your agent — see
> `integration/README.md`.

## You have persistent memory (use it every session)

This project is backed by **Helixir**, a reasoning-aware memory exposed as MCP
tools (`mcp__helixir-local__*`). You are NOT stateless: treat it as your own
long-term knowledge and keep it current. Always pass a **consistent `user_id`**
so the memory stays coherent and personal search is scoped. `claude` below is a
PLACEHOLDER — establish YOUR id first (next section) and use it everywhere.

### Establish your identity (BEFORE the first recall)
The `user_id` is YOUR name in the memory — the wrong one recalls another agent's
memories as your own. Pick ONE stable id, in this order: (1) an id you were
explicitly assigned/configured; else (2) your own name from your system prompt
(e.g. "You are Zeroclaw…" → `zeroclaw`) or, in a shell, the OS user (`whoami`).
Use that SAME id for the first `search_memory` and every `add_memory`. If recall
is empty/thin and you're unsure, call `list_users` to see who exists and confirm
your id — never silently adopt another agent's. Replace every `claude` below.

Helixir stores typed facts in a knowledge graph with causal edges, so it returns
*why* things are true — not just similar text. The read path makes no LLM calls
and is fast; search liberally instead of guessing or asking the user to repeat.

### 1. Recall first — before answering any non-trivial request (and after a summary)
Call `search_memory(query="<the user's topic>", user_id="claude")`. If it
returns nothing for your user_id, retry once with `scope="collective"`. Build on
what you find rather than re-deriving known decisions. **After a context
summary / compaction, recall first too** — the summary is lossy, the memory is
the ground truth; refresh from Helixir before continuing.

### 2. Capture durable facts — proactively, as you work
When the user states or you establish a **decision, preference, goal,
constraint, outcome, or gotcha**, store it:
`add_memory(message="<one plain sentence>", user_id="claude")`.
- `needs_clarification` in the result → the charter blocked a silent conflict
  (e.g. a reversed preference). **Ask** the `suggested_question` or apply a
  standing rule; never overwrite silently.
- `ok:true` → success, never retry. `deduped` set with `memories_added=0`
  (`saved>0`) → already known (success, not failure).
- `{ok:true, status:"accepted", pending_id}` → buffered write still finishing;
  success, searchable in seconds. Only `ok:false` (`status:"failed"`) is a failure.
- Don't store ephemeral chatter, secrets, or anything derivable from code/git.

### 3. Record outcomes AT the milestone, not at session end
The trigger is an event, not a schedule: a fix landed, a test went green, a
release shipped, a decision was made, a dead end was proven — `add_memory`
it IN THAT MOMENT, one plain sentence with the what and the why. Sessions
get cut off; a capture postponed to "the end" is a capture lost. If you
just finished explaining an outcome to the user, that explanation is the
memory — write it before moving on.

### 4. Reason with FastThink for multi-step analysis
The trigger: the moment your plan is "search_memory, then decide" — open a
FastThink session and do both inside it. Comparing options, diagnosing a
cause, any judgement resting on recalled facts qualifies; a single plain fact
does not (just `add_memory` it). What you gain over thinking silently:
`think_recall` lands stored facts inside the reasoning tree, and
`think_commit` persists ONE conclusion with SUPPORTS provenance edges from
that evidence (fast — seconds), so the next session inherits the WHY.
Flow: `think_start → think_add (build the tree) → think_recall →
think_conclude (required) → think_commit` (once). Reuse one `session_id`;
`think_discard` if it led nowhere.

### 5. You are part of a swarm
Pass your stable `agent_id` on every `add_memory` — presence in the shared
roster comes free with the write. `swarm_status` shows who else is working
this memory right now (and exposes forgotten daemons); `list_users` orients
identities. Watch `pending_outcomes`: `contradiction_review` means a dispute
touches YOUR memory — settle it with `resolve_contradiction`
(confirm/retract/preference, all non-destructive); `ops_alert` is the
memory's own health watchdog speaking — relay it to your human.

### Pick the right retrieval tool
- WHY / rationale → `search_reasoning_chain` (`chain_mode="causal"`).
- How are A and B related → `connect_memories` (anchors = query or `memory_id`).
- Only goals / one ontology type → `search_by_concept` (`concept_type`).
- General recall → `search_memory`; bulk/audit → `list_memories`.

**Principle:** the memory does not gaslight its owner — surface
`needs_clarification`, never silently overwrite a conflicting fact. Recall before
you re-derive. One identity (same `user_id`). Write durable facts, not trivia.

### Reading what the memory returns

- Results are **curated, not raw**: capped at the top-K by score, duplicates
  and same-story repeats folded. A result whose metadata carries
  `collapsed: [ids]` is one story shown once — the folded ids stay reachable
  if you need exact wording. A thin recall means "ask sharper", not "empty".
- Some BECAUSE edges are **stitched hypotheses**: a background pass (Lachesis)
  connects old memories retroactively and tags the edge `lachesis-stitch`.
  Present those as "the memory suspects a causal link", never as settled fact.
- `think_status` returns `thoughts_left` — your headroom before the session
  cap. `think_conclude` always works, even at 0: the conclusion is the exit.
- **Language note**: memories may be stored in English even when the source
  conversation was not (extraction historically normalized). If a recall in
  the conversation's language comes back thin, retry the same query in
  English before concluding anything.
- **Write for the graph**: explicit connectives build guaranteed typed edges —
  "X because Y" → BECAUSE, "X is part of Y" → PART_OF, "X is a kind of Y" →
  IS_A (English and Russian both work). Prefer stating causes and structure
  explicitly over implying them: the graph cannot see inside an atom.
- **Time windows and flashbacks**: to recall a period, pass `time_from` /
  `time_to` (RFC3339 or `YYYY-MM-DD`) to `search_memory`. The window bounds
  direct answers by EVENT time; memories outside it that are linked to
  in-window results still return with `flashback: true` and their
  `event_date` — associations across time, like human memory. Present a
  flashback as dated context ("related, from 2025-05"), never as an event
  of the requested period.
