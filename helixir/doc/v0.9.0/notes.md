# v0.9.0 — The Curation Release

> _Supersedes `v0.8.0`. The memory learns that the scarcest resource is not
> storage — it is the reader's attention._

Until now Helixir optimized for **remembering everything**. This release
optimizes for **presenting the right amount**: every read surface got a
budget, every repeat got folded, and "why" questions got a better aim. The
lot was driven by first-person dogfooding — an agent researching Helixir
through Helixir, hitting every rough edge personally — and by one economic
observation: an answer that misses forces a re-query, so the tokens are
spent twice.

## Honest limits everywhere (#81)

`think_recall` once pulled **114 facts into one reasoning session** — most
of an agent's context window on a single grounding call (a live zeroclaw
choked on exactly this). The cap existed; the contextual search branch
simply never applied it to the expansion-inflated result set, and duplicate
rows (the same memory arriving as a seed AND an expansion child) ate slots
of whatever window remained.

- All traversal branches now **dedup by memory id, then clamp** to the
  requested limit — the honest top-K.
- The recall path adds a belt (`fast_think.max_recall_results`) and a
  score floor (`fast_think.recall_min_score`, default 0.6 — measured: seeds
  live at 0.68–0.99, the expansion tail flattens at 0.41–0.55).
- Same query, same store: **8 facts instead of 114**. Ranking got *better*,
  not worse: golden MRR improved 0.825 → 0.858 because duplicates no longer
  crowd the window.
- `think_status` now reports `thoughts_left`; `think_conclude` works even
  at 0 — the conclusion is the exit, not another thought.

## Family collapse (#82)

A multi-fact message stores its atoms AND the raw source; a search matching
the story used to bill the reader twice. The write path now wires atom→raw
`PART_OF` edges, and search collapses the family into its best-ranked
member — the folded ids stay reachable under `metadata.collapsed`.
Compaction of redundancy, never of content: sibling atoms (distinct facts)
always survive, and a kept raw contains its atoms' text verbatim.

## Causality grows from both ends (#83)

The graph audit showed BECAUSE edges dense within one write and starving
between writes (RELATES_TO 82 vs BECAUSE 16) — so "why" questions resolved
to same-write mechanics instead of verdicts. Three fixes close the loop:

- **The walker prefers logical edges.** Guided hop selection used cosine
  alone, and a semantically close generic neighbor reliably outranked the
  BECAUSE edge on the same node. Typed hops (BECAUSE/IMPLIES/CONTRADICTS)
  now take priority; cosine selects within them.
- **Chain seeds aim before walking.** Seeds overfetch and candidates
  carrying causal edges win — a seed without them can only yield mechanics,
  the agent re-queries, the tokens are spent twice.
- **Lachesis gains a second duty: retroactive causal stitching.** A bounded
  pass proposes entity-overlapping pairs of OLD memories, a conservative
  LLM judge confirms explicit causation, and survivors become BECAUSE edges
  tagged `lachesis-stitch` — hypothesis-grade provenance per the apophenia
  guardrail. Capped per pass (the OOM flood lesson), convergent by
  construction (linked pairs are skipped), running as orchestrator stage 3
  on its own daemon cadence (`moira.daemon.stitch_every_passes`).
- Bonus guards: cross-write causal `relates_to` (the decision prompt gained
  a causal worked example — live-verified: two separate `add_memory` calls
  produce a BECAUSE from effect to days-older cause) and a causal 2-cycle
  guard at the single choke point under every edge writer.

## Swarm roster hygiene (#84)

One-shot agents never say goodbye — 24 of 27 roster rows were zombies stuck
on "working" for days. `swarm_status` now hides agents silent past
`swarm.presence_ttl_secs` (default 30 min, deliberately above the daemon
pass interval; `0` disables) and reports `hidden_stale`. The Agent node is
never deleted: it anchors `AGENT_CREATED` authorship provenance.

## Prompts caught up with the machine

Tool descriptions, the cognitive protocol, the server instructions and both
integration templates now teach what agents would otherwise misread:
`metadata.collapsed` means folded-not-lost, `lachesis-stitch` edges are
suspected links (present them as hypotheses), a thin recall means *ask
sharper*, and contradiction flags are settled explicitly with
`resolve_contradiction`, never ignored.

## Also

- Every feature above ships with a permanent net: `raw_family_e2e`,
  `stitch_e2e` (convergence proven), a causal 2-cycle e2e, config-plumbing
  units. The golden read-path oracle caught three latent defects during
  this wave's surgery — none of them were in any ticket.
- 153 lib tests; the full read/write/chain oracle set green.

## Upgrade

Drop-in. New config keys are optional with safe defaults. Old raw sources
(written before v0.9.0) carry no family edges, so collapse benefits new
writes; a backfill pass is tracked in #82.
