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

### 3. Record outcomes at the end of each meaningful step
After a fix, decision, or milestone, write what was decided/done so the next
session inherits it.

### 4. Reason with FastThink for multi-step analysis
For non-trivial analysis/planning/decisions, use the scratchpad rather than many
small writes: `think_start → think_add (build the tree) → think_recall
(optional) → think_conclude (required before commit) → think_commit (persists;
heavy, call once)`. Reuse one `session_id`; `think_discard` if it led nowhere.

### Pick the right retrieval tool
- WHY / rationale → `search_reasoning_chain` (`chain_mode="causal"`).
- How are A and B related → `connect_memories` (anchors = query or `memory_id`).
- Only goals / one ontology type → `search_by_concept` (`concept_type`).
- General recall → `search_memory`; bulk/audit → `list_memories`.

**Principle:** the memory does not gaslight its owner — surface
`needs_clarification`, never silently overwrite a conflicting fact. Recall before
you re-derive. One identity (same `user_id`). Write durable facts, not trivia.
