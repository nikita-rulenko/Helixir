# Helixir Memory Charter

> **ACTIVE v1.0 — owner-approved through issue #34.** The write path follows
> "defer, don't destroy": destructive verdicts governed by the charter are
> deferred, both facts remain stored, and `resolve_contradiction` settles the
> dispute with history intact. C1, C2 and C4 are hard constitutional guards.
> The compatibility setting `write.charter_blocking = false` may disable
> C3/C5 deferral for diagnostics, but it cannot disable those hard guards.

This charter governs what Helixir may decide on its own when writing
memories, what it must escalate to the agent (and through the agent — to the
human), and what it must never do. Three layers, strongest first.

## 0. RBAC boundary

The charter governs memory curation, not authorization. RBAC is stored in the
HelixDB graph and is the single source of truth for CLI, MCP, and Rust. The
decision engine must never grant, revoke, or infer access: `LINK_EXISTING` and
`CROSS_CONTRADICT` preserve cross-user provenance but do not grant visibility.
`actor_id` is the authenticated principal; `user_id` is the memory owner or
target. Authorization is resolved by `RbacManager` before the curation pipeline
is called, and enabled RBAC fails closed on database errors.

## 1. Constitution (immutable — changed only by explicit human edit)

These rules are not available to charter self-learning and override
everything below.

- **C1. Never auto-delete.** **Enforced in code**: a `DELETE` verdict from
  the decision engine is executed as `SUPERSEDE` — the old fact stays in
  history with the delete-intent recorded in the supersession reason, and
  the conflict is escalated. Memory is an elder brain: it forgets nothing
  silently. (The library-level `delete()` remains as an explicit
  administrative action; it is deliberately not exposed over MCP.)
- **C2. Never overwrite memories marked `immutable`.** System seeds and
  explicitly adopted charter rules are promoted to immutable storage. The
  current agent-facing API does not expose a general-purpose "mark final"
  operation; operators may promote existing records through the controlled
  persistence path.
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

Rules appear in the live `memory://rules` resource when the user explicitly
adopts an agent proposal after repeated identical contradiction resolutions.
Precedent episodes retain SUPPORTS provenance to both disputed memories.
Adopted rules are marked immutable and render beside this constitution; the
constitution itself never self-learns.

_(Runtime rules are loaded from HelixDB and appended by `memory://rules`.)_

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

## 4. Executable contract

| Article | Runtime enforcement | Verification |
|---|---|---|
| C1 | `DELETE` is converted to non-destructive deferral/supersession | deterministic charter tests + blocking E2E |
| C2 | direct and decision-pipeline rewrites load `Memory.immutable` and fail closed; seeds/rules are created immutable atomically and interrupted seed passes resume | deterministic protection tests + protected-memory E2E |
| C3 | preference/goal/opinion rewrites enter `needs_clarification`; destructive verdicts defer by default | deterministic charter tests + reversal E2E |
| C4 | direct and decision-pipeline rewrites reject `source=raw_input` targets | deterministic protection tests + protected-memory E2E |
| C5 | destructive verdicts below the configured confidence threshold escalate and defer by default | deterministic charter tests |
