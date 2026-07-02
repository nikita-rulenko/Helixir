# Changes — generative-memory agents: the `agents/` layer (2026-06-14)

Branch `feat/daemon-multiclient`. (`main` untouched.) Helixir stops being only an
MCP server and becomes an **agent** whose MCP surface is one part: a new
`src/agents/` layer holds background agents that *compose* toolkit primitives.
Dependencies flow `agents → toolkit`, never the reverse.

## Highlights

### New layer: `src/agents/` (agents as libraries)
- **Clotho** — the tagging agent. `HelixirClient::clotho()`.
- **Lachesis** — the chain-routing + coherence-gate agent. `HelixirClient::lachesis()`.
- Layering rule: agent *policy* lives in `agents/<name>/`; the *capabilities* it
  drives stay in `toolkit/` (primitives only). See `architecture.md` §7.7.

### Category dictionary + Clotho auto-tagging (#33)
- Schema: `Category` nodes, `SUBCATEGORY_OF` (hierarchy), `ALIAS_OF` (synonyms),
  `TAGGED_AS` (`Memory → Category`), `CategoryEmbedding` (`d8edc85`).
- `toolkit` primitives: `ensure_category`, `link_subcategory`, `tag_memory`,
  `embed_text`, `category_member_ids`.
- `Clotho::{seed_dictionary, auto_tag}`: a controlled English-canonical vocabulary;
  `auto_tag` matches a memory against it by **in-memory cosine** (SearchV exposes
  no readable score — see HQL note below), tags over a threshold, propagates
  ancestors, and **escalates per the charter** when nothing fits (no silent
  invention). Live: nomic cosine cleanly separates domains (on-target ≈0.74 vs
  off-target ≤0.57); bar 0.65 → precise single-domain tags.

### Cross-domain bridges (#33) — the third axis over the flat graph
- `connect_memories` routes a second axis: memories sharing a category bridge
  through the `Category` node (`Memory → TAGGED_AS → Category → In TAGGED_AS →
  Memory`) — no pairwise edge materialised (hubs don't explode). Purely additive:
  a memory with no categories yields no bridge.

### Longest-chain context reconstruction (#47)
- `HelixirClient::longest_chain(topic, max_hops)`: grow a capped reasoning
  ego-network from topic seeds (reusing `fetch_level`), DFS the single longest
  **simple** path, ranked by hops then cumulative confidence. Returns an ordered
  `ChainNarrative` (steps + edge types + per-edge weights + `created_at` +
  confidence). Cycle-guarded, node/step-budget capped. Live: a 6-hop thread
  reconstructs the project's own development history.

### Per-edge weights now flow through the read path (#33, "real weights")
- The writer assigns each reasoning edge a `strength`/`probability` (0–100), but
  the read path had dropped it for a per-FAMILY constant. Now `family_weight ×
  strength_norm` drives PPR ego-edge mass + the child's `graph_score` in batched
  expansion, and `connect_memories` path confidence. Conservative: unweighted
  (legacy) edges multiply by 1.0; phase-1 vector/BM25 hits untouched. Search MRR
  holds at 0.703 (hit@5 100%) — no regression.

### Lachesis coherence gate — the apophenia guard (#39)
- `assess(edges)` (pure, unit-tested): **coherence** = the *geometric mean* of a
  chain's edge weights (length-fair per-hop quality) + **reasoning support** =
  fraction of hops on a typed reasoning edge vs a bare association bridge. A
  survivor is a `PlausibleHypothesis` flagged **requires_verification** — Lachesis
  proposes, never adjudicates. Otherwise `LikelyApophenia`.
- `Lachesis::route(a, b)` = `connect_memories` → `assess`. Live: an
  oracle→#43→…→#33 pair routes a 4-hop all-reasoning chain, coherence 0.622 →
  hypothesis.

### Lachesis PMI subset-overlap routing (#39) — apophenia by arithmetic
- `pmi(|A|, |B|, |A∩B|, N) = ln(|A∩B|·N / (|A|·|B|))`: co-occur more than chance
  → > 0 (real, surprising); chance → 0; never → −∞. A **thick** subset's large
  cardinality sits in the denominator, so it **gates itself out** with no LLM.
  `Lachesis::subset_pmi(a, b, universe)`. Live: a controlled universe measures
  PMI(specific) = 1.0986 vs PMI(thick) = 0.0000.

## Operational notes / HQL lessons (recorded for future work)
- **SearchV returns nearest-first order but NO readable score** — `distance` is
  skipped in HelixDB's JSON serialization, and raw vectors aren't serialized
  either. Thresholded matching over a small controlled dictionary is therefore
  computed in Rust (cosine), not in the DB.
- **`AddE ::From(x)` needs a node looked up by field**, not an application id
  string (`ID` params expect the internal UUID). Fixed in `addEntityEmbedding` /
  `addCategoryEmbedding` (`65fa207`), mirroring the working `tagMemoryWithCategory`.

## Verification
Every increment gated: lib + `-D warnings`; the deterministic e2e for the
feature; and, for anything touching the read path, the liveness oracle (L1 17/17
+ L2) + the cross-domain/connect tests + the MRR read-path tests. New permanent
suites: `clotho_autotag_e2e`, `cross_domain_bridge_e2e`, `longest_chain_e2e`,
`lachesis_gate_e2e`, `lachesis_pmi_e2e`, plus 10 Lachesis unit tests.

---

## Update — the full pipeline + operability (later in 2026-06)

The agents matured into an end-to-end, operable pipeline:

- **Lachesis** completed: a coherence gate (geometric-mean edge weight × reasoning
  support), **PMI subset-overlap routing** (`ln(|A∩B|·N/(|A|·|B|))` — a thick axis
  gates itself out; one number = apophenia gate = surprise = specificity),
  `route_subsets` for end-to-end cross-domain threads, and **drill-to-anchors**
  (every hop carries its witness memories — provenance is what makes a hypothesis
  falsifiable).
- **Clotho** grew a self-building dictionary (LLM mints a category on a miss) and a
  **dominance gate** (tag only within a margin of the best match) that kills the
  noise-floor cross-tags the provenance drill exposed.
- **Atropos** (#48): curates Lachesis threads into ranked, deduplicated `Insight`s
  with provenance and a `proposed → verified → refuted` lifecycle.
- **Orchestrator** (#41): one `full_pass` runs Clotho → Lachesis → Atropos —
  choreography kept separate from scheduling.
- **Daemon** (#42, #49): schedules `full_pass`; `helixir daemon start/stop/status`
  runs it **detached** (setsid + PID file in `~/.helixir`).
- **CLI + onboarding**: the `helixir` binary drives + monitors the agents with
  activity/insight journals; **`helixir setup`** (#50) wires the MCP server into
  agent clients (Claude Code / Desktop / Cursor / Gemini CLI), non-destructively.

**Capstone (#48):** the whole pipeline, run on clean controlled data, reconstructs
the guar chain *weather → crop → thickener → fracking → price* as a single
provenance-carrying insight — `clean-in → signal-out`. On a real (mono-domain,
noisy) corpus it faithfully curates noise, which the provenance exposes on sight:
**output quality equals corpus/tag hygiene, not code.**
