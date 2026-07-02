# v0.6.0 — the hive release

> _Supersedes `v0.5.0`. The theme: the hive is real — agents trust their
> writes, find their identities and each other through the DB, the graph
> substrate holds under audit, and generated knowledge flows back into
> the memory it came from._

v0.5.0 made memory collective. v0.6.0 makes it a **working hive**: four
independent zeroclaw agents wrote overlapping knowledge from different
angles and the system converged it — consensus without gaslighting,
contradictions surfaced not merged, and the Moirai spun a 5-hop
cross-domain insight (weather → agriculture → petrochemicals → battery
tech → EV market) out of what the swarm wrote. Every claim in this list
was verified against the live database, not just the test suite.

## Write-ack agents can trust (#63)

- **Confirm-or-promise contract**: buffered `add_memory` now waits
  briefly for the pipeline (`ingest.ack_wait_ms`, default 8 s). If the
  write finishes in time the agent gets the real result inline
  (`ok:true` + `memory_ids`); if not, an explicit promise
  (`{ok:true, status:"accepted", pending_id}`) pollable via
  `get_add_status`.
- **One-bit success**: every result carries a top-level `ok`. Only
  `ok:false` is a failure; `deduped` non-empty with `memories_added=0`
  means "already known" — success. In the hive finale, 3/3 then 4/4
  agents trusted their writes with zero retry storms (the v0.5.0 blocker
  that motivated this release).

## Identity & rendezvous (#64, #39)

- **`list_users`**: a privacy-safe roster of identities (ids/names, no
  content) so an agent can find its own or a teammate's `user_id`
  instead of guessing. Collective-gated; Solo returns
  `available:false`.
- **`swarm_status`**: the live agent roster — role, host, status,
  seconds since last heartbeat — read straight from the shared graph.
- **Auto-heartbeat**: `add_memory` with `agent_id` stamps presence
  (host, status, `last_seen`) on the Agent node as a side effect of
  writing. Rendezvous through the database itself: no side channel
  exists or is needed.
- **Identity discipline**: templates and seeds now say it loudly — pick
  your OWN stable `user_id`; the literal `claude` in examples is a
  placeholder to replace (agents were copying it verbatim and
  inheriting someone else's memory).

## The typed-edge arsenal actually fires (#66, #46, #62, #23)

- **Seven relation types on the write path**: causal `IMPLIES`,
  `BECAUSE`, `CONTRADICTS`, `SUPPORTS` plus associative `RELATES_TO`,
  `PART_OF`, `IS_A` — all riding one `MEMORY_RELATION` edge whose
  `relation_type` names the type (no schema change per type). The
  extractor got a numbered stop-at-first decision procedure with worked
  examples; unknown tokens degrade to `RELATES_TO`, never to a false
  `IMPLIES`.
- **Ontology metadata reaches the decision prompts**: neighbour
  candidates now carry their `Type:`, and `decision.relates_to` is
  persisted — cross-memory associative edges from a fresh write.
- **SAME-SUBJECT GATE (#46)**: destructive verdicts (UPDATE / SUPERSEDE
  / CONTRADICT / DELETE) are only legal for the same specific subject;
  keyword overlap yields ADD + `RELATES_TO`. Stops "my cat is black"
  from superseding "my dog is black".
- **DB-verified edge tests**: a new e2e family walks the database after
  the write and asserts the edge *types* that landed — not just that the
  tool returned 200.

## Substrate integrity (the audit wave: #67, #68, stale embeddings, AGENT_CREATED)

A ground-truth audit of the live store (INSTANCE_OF coverage, edge
reality, history) confirmed the claims and surfaced four real defects —
all fixed:

- **Ontology self-heal (#67)**: retry-amplified seeding had quadrupled
  the concept tree (80 = 4×20). Boot now dedupes duplicate trees keeping
  the earliest node per `concept_id` (`dropConceptByInternalId`), and
  seeding is idempotent under decode-error retries.
- **Merge over-conflation (#68)**: `raw_input` nodes are excluded from
  BOTH sides of NLI paraphrase-merge pairs — a verbatim raw memory can
  no longer transitively bridge two distinct atoms into one group.
- **Stale-embedding leak**: `update_memory` deleted the old embedding by
  external id against an internal-UUID query — the delete silently
  failed and searches kept matching pre-update text. Resolve-then-delete
  fixed; bench delete-errors dropped to zero.
- **AGENT_CREATED finally fires**: Agent→Memory provenance edges were
  never written for unregistered agents. Ensure-then-link: the Agent
  node is auto-created on first sight.

## Generative memory closes the loop (#38, cadence)

- **Insights persist as memories**: every curated hypothesis is stored as a
  first-class `opinion` memory (certainty 40 — a lead, not a truth)
  under `user_id=helixir`, with `SUPPORTS` provenance edges from every
  witness memory. Any agent can now *recall* generated knowledge the
  same way it recalls stored facts. Idempotent by content key.
- **Per-Moira cadence**: `--clotho-every / --insight-every /
  --merge-every / --reconcile-every` on the daemon (and
  `moira.daemon.*_every_passes` in config): tag every pass, route
  insights every Nth, `0` disables a stage.
- **Intensity knobs in config**: `[moira.clotho]` grow/tag thresholds,
  `[moira.lachesis]` PMI/coherence bars + DFS budget,
  `[moira.atropos]` `quality_pmi_bar` — curation strictness is an
  operator dial, not a constant.

## FastThink commit is finally fast

Live agent feedback: `think_commit` took 40–96 s. The cause was
algorithmic, not model speed — commit glued the session's conclusions
into a text blob (with `[Evidence: mem_...]` pasted into the content)
and paid the full extraction pipeline to re-discover structure the
session already held. Fixed at the root:

- **No re-extraction**: conclusions enter the pipeline as prepared
  atoms (`add_prepared` — type, certainty, importance stamped from
  config). Dedup, the charter and typed-edge enrichment still apply;
  a novel conclusion commits with **zero LLM calls**. Measured:
  **2.7 s** on the same setup that took 40–96 s.
- **Evidence became provenance**: recalled memories now `SUPPORTS`-edge
  the committed conclusion instead of polluting its content.
- **Entity discovery moved off the critical path**: one background
  extraction call links entities after the agent has its ack.
- **Wall-of-text escape hatch**: conclusions over
  `fast_think.commit_extract_over_chars` (default 900) still get full
  extraction — atomizing a blob is worth the wait.
- **Bonus for every write**: per-atom relation inference (one LLM call
  per stored atom!) used to run *sequentially* inside the store loop;
  it now runs concurrently, so multi-atom `add_memory` pays the slowest
  call, not the sum.

## Self-documentation (#35, #40, #41)

- **The manual lives in the memory**: install-time self-seed
  (`HELIXIR_SELF_SEED`, written by `helixir setup`) stores the operating
  manual — modes, connection recipes for Claude Code / Desktop / Cursor
  / zeroclaw, identity rules, the ok-contract, FastThink usage, data
  safety — under `user_id=helixir`. Versioned (`helixir-seed@N`),
  idempotent, verbatim (no LLM extraction).
- **GLOSSARY.md**: the project vocabulary defined once — PPR, RRF, PMI,
  apophenia gate, content key, same-subject gate, the Moirai, the guar
  chain — and seeded into the graph too (seed@3, 51 facts): ask the
  memory how to use the memory.
- **Docs re-verified against code**: README edge tables now match
  reality (the arsenal rides `MEMORY_RELATION`; `SUPERSEDES`/
  `CONTRADICTS` are the decision engine's dedicated edges; `OCCURRED_IN`
  honestly reserved), data-model/userflow re-pinned at v0.6.0, 19 MCP
  tools, 153 HQL queries.

## Deploy & ops discipline

- **`make install`**: binaries go to `~/.helixir/bin`; agents never run
  `target/release` (a cargo rebuild under a live MCP session gets the
  running process killed by macOS — learned the hard way, encoded in
  the Makefile).
- **MCP prompt overhaul for weaker models**: `add_memory`'s result
  contract rewritten as short bullets (a DeepSeek-class model must not
  retry an `ok:true`), tool descriptions carry when-to-use routing.
- **Autobackup ticketed (#65)**: bench data loss traced to data-dir
  recreation, documented; scheduled backup is deliberately deferred —
  feature correctness first.

## Verification

- 133 unit tests green; fmt/clippy CI green on every push of the wave.
- Full-surface liveness oracle drives all **19 MCP tools** through the
  real stdio transport with write→read-back asserts (Solo pinned via
  env — the host config may set a collective tier).
- Rendezvous e2e: a writer with `agent_id` appears ACTIVE (with host) to
  a second, independent MCP consumer.
- Hive finale: 4 zeroclaw agents, one task from different angles with
  deliberate overlap — all writes `ok:true`, cross-agent recall
  confirmed, 3 duplicate-angle writes converged to `LinkExisting`, 1
  genuine conflict surfaced as `Contradict`, 47 divergent-number merge
  candidates blocked by the NLI judge, and the pipeline generated the
  5-hop weather→EV insight with full witness provenance.

## Known gaps (honest list)

- `read_path_e2e` / `mcp_read_e2e` golden fixtures pinned exact memory
  ids from the historic bench corpus lost on 2026-06-30; both suites now
  SKIP with an explicit message until the golden set is re-recorded.
- Collective-collapse e2e (#3a) retries up to 3× — per-write extraction
  is probabilistic, and an attempt whose atoms come out only as
  paraphrases never exercises the collapse (the paraphrase case belongs
  to the async NLI merge backstop).

- Category dictionary carries e2e pollution (`thick-*`/`crop-*` names);
  cleanup needs a `dropCategory` HQL on the next schema swap.
- Contradiction-review notices are re-delivered after resolution
  (cosmetic, mini-ticket).
- Insight quality tracks tag/corpus hygiene on a real biased corpus —
  by design the provenance is the signal/noise discriminator, but the
  hygiene tooling is thin.
- Scheduled backups (#65) deferred; deploy playbook requires a manual
  `data.mdb` copy first.
