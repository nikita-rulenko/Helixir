# Helixir Memory Charter

> DRAFT v0.1 — awaiting owner approval. The write path currently runs in
> "flag, don't block" mode: conflicts listed below are surfaced to the agent
> in `add_memory.needs_clarification`, but every decision still executes.
> Blocking semantics activate only after this charter is approved.

This charter governs what Helixir may decide on its own when writing
memories, what it must escalate to the agent (and through the agent — to the
human), and what it must never do. Three layers, strongest first.

## 1. Constitution (immutable — changed only by explicit human edit)

These rules are not available to charter self-learning and override
everything below.

- **C1. Never auto-delete.** **Enforced in code**: a `DELETE` verdict from
  the decision engine is executed as `SUPERSEDE` — the old fact stays in
  history with the delete-intent recorded in the supersession reason, and
  the conflict is escalated. Memory is an elder brain: it forgets nothing
  silently. (The library-level `delete()` remains as an explicit
  administrative action; it is deliberately not exposed over MCP.)
- **C2. Never overwrite memories marked `immutable`** (system seeds,
  approved charter rules, memories the user marked final).
- **C3. Preferences, goals and opinions are never rewritten silently.**
  Any `CONTRADICT`, and any `UPDATE`/`SUPERSEDE` touching these types —
  even at high engine confidence — is escalated. A reversed preference may
  be a real change of mind, a different project context, or an extraction
  error — only the human knows which.
- **C4. `raw_input` memories are never modified or superseded.** They are
  the source of truth that survives extraction mistakes.
- **C5. Low-confidence destructive operations escalate.** `UPDATE` /
  `SUPERSEDE` with decision confidence below 70 is flagged for review.

## 2. Learned rules (grown from precedents, each with provenance)

Rules appear here when the user explicitly answers "allow always" to a
clarification, or approves an agent proposal after repeated identical
answers. Every rule links (BECAUSE edges) to the episodes that created it.

_(empty — no precedents yet)_

<!-- Example of a learned rule:
- **L1.** Facts about code structure: newest wins silently (no escalation
  on UPDATE/SUPERSEDE). Born from precedents mem_xxx, mem_yyy, mem_zzz.
-->

## 3. Defaults (thresholds; tunable in config)

- Cosine ≥ 0.98 against an existing memory → exact duplicate → `NOOP`,
  silent.
- Cosine < 0.70 against everything → genuinely new → `ADD`, silent.
- `CONTRADICT` / `CROSS_CONTRADICT` decisions → execute (both facts are
  kept, linked by a CONTRADICTS edge — non-destructive) **and** flag in
  `needs_clarification`.
