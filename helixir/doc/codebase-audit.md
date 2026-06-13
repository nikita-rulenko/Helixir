# Codebase audit — 2026-06-13

**Why:** before committing to the memory-provider / daemon architecture (#42), take
stock of the codebase. It feels large and sprawling with a lot of "suspended"
work; we want to know what is dead, what is parked, what is fragile, and what
actually blocks or enables the transition to *one daemon per machine, many
agents*.

**Method (the methodology we hold to):** every finding here is grounded in a
concrete probe (grep/wc/call-site trace), not vibes.

**Liveness oracle — compilation is NOT proof of life.** A finding that something
is dead/safe-to-delete is confirmed only when the *behaviour* of the product is
unchanged: all e2e green, memory actually writes and reads back, and **every MCP
Helixir function** is exercised through the transport. "Still compiles after
deletion" only proves the code was *referenced* in the build graph — a dead twin
compiles fine. So we first build a full-surface MCP e2e (the oracle), then gate
every deletion stage on it being green, not on `cargo check`. Compilation is
necessary, not sufficient.

**Snapshot:** branch `audit/codebase-health`, ~27.5k LOC of `src/`, 130 HQL
queries defined (0 dangling), only 5 TODO/FIXME markers (so the cruft is in
*dead/parked modules*, not in marked debt).

Status legend: `confirmed` · `suspected` · `parked-intentional` · `done`

---

## 0. Liveness oracle — current MCP-surface coverage · BLOCKER for deletion

We can only classify "live" once every MCP function is exercised e2e. Current
coverage of the 17 tools (grep across `tests/`):

**Covered (8):** add_memory, search_memory, get_add_status, get_memory_graph,
search_by_concept, search_reasoning_chain, connect_memories, (search_memory).

**NOT covered (9):** list_memories, update_memory, search_incomplete_thoughts,
and the **entire FastThink family** — think_start, think_add, think_recall,
think_conclude, think_commit, think_discard, think_status.

- **Implication:** FastThink (~845 LOC: `fast_think/manager.rs` 464 +
  `session.rs` 381) is wired as MCP tools but **never exercised by any e2e** —
  its liveness is currently unknown. Same for list/update/incomplete-thoughts.
- **Action (must precede any deletion):** build a full-surface MCP e2e that
  drives all 17 tools through the transport + verifies write→read-back
  persistence. That is the oracle every deletion stage is gated on.

## A. Dead / parked code (the "sprawl")

**Two tiers of "dead" — they need different proof:**
- **Tier 1 — not in the binary** (module never declared via `mod`). The compiler
  never includes these, so they *cannot* be exercised by the oracle — dead by
  construction. Deleting them cannot change product behaviour; the oracle run
  after deletion just guards against a wrong "undeclared" call (e.g. a hidden
  `#[path]`). ~1863 LOC.
- **Tier 2 — compiled but no live caller** (declared/re-exported, never invoked).
  Here "compiles" proves nothing — only the full MCP oracle proves the live
  paths don't use it. ~1355 LOC.

### A1 — `src/toolkit/analytics/` (450 LOC) — DEAD, not even compiled · confirmed [Tier 1]
No `mod analytics;` exists in any `mod.rs` (`lib.rs`, `toolkit/mod.rs`,
`tooling_manager/mod.rs` all clean). The files sit on disk but are **outside
the compilation unit** — they aren't type-checked, can't be called, and rot
silently. `manager.rs` alone is 437 LOC.
- **Impact:** pure dead weight; misleads anyone reading the tree into thinking
  analytics is a feature.
- **Action (proposed):** delete, or if the intent is real, wire `mod analytics`
  and give it a caller + a test. Decide with Nikita.

### A2 — `src/core/services/{linking,resolution}` (~1355 LOC) — suspected dead twin · suspected
`pub mod services` IS declared (so it compiles and is type-checked), and
`core/mod.rs` re-exports `LinkBuilder, LinkBuilderStats, ResolutionStats`. But a
call-site trace finds **no `LinkBuilder::new` / `ResolutionService::new`
anywhere outside the module** — only the re-export and doc comments. The *live*
entity-linking path is the add pipeline
(`tooling_manager/add_pipeline/enrich.rs` → `entity_manager.link_to_memory`,
`resolve_and_persist_extraction_relations`), which does NOT use
`core/services`.
- **Impact:** ~1.3k LOC of a parallel "twin" subsystem, compiled but unreached —
  the same shape as the already-dead `integrator/`. Doubles the mental model of
  "how does linking work".
- **Action (proposed):** confirm zero live callers (trait-object/dyn dispatch
  too), then delete or fold; if it's the *intended* future linking engine, say
  so explicitly and add the wiring task.

### A3 — `src/toolkit/mind_toolbox/integrator/` — known dead twin · parked-intentional
Already commented out in `mind_toolbox/mod.rs` with a rationale block ("dead
twin of the live add pipeline... kept on disk as historical reference"). It is
the only consumer of the duplicate `cosine_similarity` (D1) and the
ReasoningEngine naming collision (D4) per `doc/duplication-audit.md`.
- **Action (proposed):** if it's truly reference-only, move it out of `src/`
  (e.g. `doc/` or delete — git is the history). Leaving disabled code in `src/`
  is the cruft we're auditing for.

### A4 — `tooling_manager/helpers/reserved.rs` — parked query wrappers · parked-intentional
Documented as "helix queries that exist DB-side but not yet invoked... removing
them is a public-API regression (schema shared with HelixDB)". Parked surfaces:
`link_memory_to_session`, `link_agent_to_memory`, `add_entity_relation`,
`add_entity_part_of`, `add_memory_valid_in_context`.
- **Not rot — strategically important:** `link_agent_to_memory` /
  `link_memory_to_session` are exactly the **multi-agent primitives the #42
  daemon needs**. The schema already anticipates the provider model; the code
  just hasn't wired it. Tie these to #42 rather than treating them as debt.

---

### A5 — `mind_toolbox/memory/` dead cluster + `misc_toolbox/` — not in binary · confirmed [Tier 1]
`memory/mod.rs` declares only `context, crud, evolution, models, retrieval`. The
following sit in the dir but are **never declared → not compiled → not in the
binary**: `contradiction.rs` (51), `relations.rs` (168), `supersession.rs` (155),
`user_link.rs` (74), `deletion/` (545), `remark/` (419). Plus `toolkit/misc_toolbox/`
(~empty).
- **The tell:** the live DELETE→SUPERSEDE logic (release-blocker #20) lives in
  `llm/decision/models.rs::supersede()` + `core/charter.rs` +
  `tooling_manager/crud.rs` — **not** in `memory/supersession.rs`. So these are
  old copies left on disk after the logic moved; the real implementations
  superseded them.
- **Action:** Tier-1 deletion candidates — removing them cannot change behaviour
  (not in the product). Confirm with a full oracle run after deletion.

---

## B. #42 (daemon) readiness — what blocks vs enables

### B1 — Global state is modest and daemon-friendly · confirmed
Only process-global state: `OnceLock` cached `RetrievalProfile`
(`core/retrieval_profile.rs`), `OnceLock` broadcast channel for ingest push
(`ingest_buffer.rs`), and `lazy_static` regex/level tables. No `static mut`. In
a daemon these become shared-once-per-process — correct by construction. The
ingest broadcast channel even *improves*: one daemon → the best-effort push
reaches every connected agent.

### B2 — Ingest worker is spawned per-process — the #25 serial hole · confirmed
`client.initialize()` spawns `run_ingest_worker` when the buffer is on. With N
client processes that's N workers draining one shared `PendingInput` queue + N
startup orphan-recoveries racing. The "single serial worker" invariant holds
only *within* a process. **The daemon makes it a true singleton** — this is the
core reason #42 fixes #25's guarantee, not just the resource crash.

### B3 — Transport is hardwired stdio · confirmed
`server.serve(stdio())` (`mcp/server.rs:105`). The whole #42 change pivots here:
add a shared transport (unix socket / HTTP-SSE) and a thin stdio shim for
backward compatibility. No other code assumes stdio specifically (the server is
built from routers + a `ToolingManager`), so the transport swap looks localized.

### B4 — The data model already has Session/Agent surfaces · confirmed
See A4 — the schema carries Session/Agent/Context nodes and edges, with query
wrappers parked in `reserved.rs`. Positive signal: the provider model is a
*wiring* job on an existing schema, not a schema redesign.

---

## C. Fragility

### C1 — 46 `unwrap()/expect()` in non-test `src/` · confirmed (needs triage)
The NaN crash (#41) came from one of these. "Never crash the elder brain"
argues for a triage pass: split into (a) provably-safe (`NonZero` consts, static
regex compilation) vs (b) on external/DB/LLM/parsed data (real panic risk).
Harden category (b) like we did the ranking sorts.
- **Action (proposed):** enumerate the 46, classify, fix the (b) set in a
  dedicated hardening pass (sibling of #41).

---

## D. Findings the liveness oracle surfaced · the oracle is built and green

`tests/mcp_full_surface_e2e.rs` (L1: all 17 tools + write→read-back) and
`tests/mcp_multi_consumer_e2e.rs` (L2: the 7 multi-consumer invariants) are now
green against the live stack. Building them already paid off:

- **FastThink is live, not broken.** L1's first run failed `think_commit`
  (empty memory_id) — but the cause was a *test* artifact: the conclusion
  duplicated a fact added earlier in the same run, so the pipeline correctly
  deduped to 0 new memories. With a novel conclusion, commit persists. So the
  845-LOC FastThink subsystem (`fast_think/`) is **confirmed live** — keep it.
  Methodology note: an assertion failure is not proof of a bug; verify the cause.
- **Cross-user consensus fragments under concurrent timing → #43.** Three
  agents writing an identical fact fragment into three `user_count=1` nodes
  instead of consolidating, because of HelixDB snapshot lag (the prior write
  isn't visible to the next writer's dedup search). Confirmed it's timing, not
  logic: visibility-gated writes consolidate. Strengthens the serial-worker /
  daemon case (#25 / #42).
- **Three e2e test artifacts in a row** (FastThink self-dup, buffer self-dup,
  id-matching) taught a reusable lesson: extractor rewording + cross-run dedup
  make id/exact-content assertions unreliable — assert by a unique entity token
  in the fact + metadata (user_count), and gate on visibility, not fixed sleeps.

## Stage log — dead code removed (oracle-gated)

Each stage: delete → `cargo build --all-targets -D warnings` → L1 + L2 oracle
green against the live stack → commit. Behaviour unchanged at every step.

| Stage | What | LOC | Commit |
|-------|------|-----|--------|
| 1 | Tier-1 not-in-binary (analytics, misc_toolbox, memory/{contradiction,relations,supersession,user_link,deletion,remark}) | ~1863 | `ec80a60` |
| 2 | `core/services/{chunking,linking,resolution}` dead twin (compiled, no caller) | ~1355 | `686b2b9` |
| 3 | `integrator/` dead twin (was `mod`-commented) | ~589 | `1b91d04` |

`src/` 27477 → 23654 LOC (−3823, ~14%). `reserved.rs` deliberately KEPT — it
parks the Session/Agent surfaces #42 will wire (A4).

## Running conclusions (for the #42 decision)

1. There is real cleanable cruft — **~1.8k+ LOC** across `analytics/` (dead) and
   `core/services/` (suspected dead twin), plus the disabled `integrator/`.
   Clearing it shrinks the surface before the daemon work.
2. Nothing structurally *blocks* #42: global state is daemon-safe, the transport
   swap is localized, and the schema already carries the multi-agent surfaces.
3. The daemon is the proper fix for #25's serial guarantee (B2), not only the
   resource crash — that strengthens the case for doing #42 before relying on
   the buffer in a multi-agent setting.

> Next probes: (i) confirm `core/services` has zero live callers incl. dyn
> dispatch; (ii) enumerate + classify the 46 unwraps; (iii) map the config/env
> surface (one daemon config vs per-process env); (iv) check the FastThink +
> analytics overlap and the `core/services/resolution` vs `add_pipeline` overlap.
