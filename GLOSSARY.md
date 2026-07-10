# Glossary

Helixir's working vocabulary — the terms used across the README, the
engineering docs (`helixir/doc/`), commit messages, and the memories the
system keeps about itself. Each entry says what the term means **here**, and
where the concept lives in the code. Alphabetical within sections.

---

## Core ideas

**Elder brain.** The design metaphor for the whole system: a memory that
never forgets, accumulates the experience of many agents, and sees
connections none of them saw individually. Practical consequences: no delete
tool (supersede + history instead), chains valued over single facts, and a
generative layer (the Moirai) that proposes new connections.

**Atomization.** `add_memory` never stores your paragraph as-is. The LLM
extractor splits input into *atomic facts* — one claim per memory — each
classified by ontology type and linked to entities. Chains and dedup only
work on atoms; a blob memory is a dead end. (`src/llm/extractor.rs`)

**The writer pays, the reader flies.** The cost asymmetry principle. All
LLM work (extraction, dedup decisions, relation inference) happens at write
time; the read path is pure math over precomputed structure — zero LLM
calls, ~15–30 ms warm. This is why a fully local setup is practical.

**Dogfooding.** The maintainers (human and agents) use Helixir as their own
long-term memory while building it. The project's decisions, gotchas and
vision are stored *in* the project, under `user_id=claude` (the working
agent) and `user_id=helixir` (the system's manual about itself). If it
doesn't work for us, it doesn't ship.

**Self-documentation.** The extension of dogfooding to docs: the operating
manual, integration recipes and this glossary are seeded into Helixir's own
graph (`user_id=helixir`), so any connected agent can *ask the memory* how
to use the memory. (`src/toolkit/tooling_manager/seeds.rs`)

**Memory charter.** A human-editable constitution
([`helixir/memory-charter.md`](helixir/memory-charter.md)) of rules the
write path may never override — chiefly: destructive or conflicting writes
are never resolved silently. Violations come back as `needs_clarification`
questions for the human.

**Anti-gaslight.** Shorthand for the charter's core promise: the memory
does not silently rewrite what its owner knows. A reversed preference, a
contradicting fact, a paraphrase with *different numbers* — all escalate
instead of overwriting. (The NLI judge blocking divergent-number merges is
the anti-gaslight backstop on the merge path.)

## Modes & the hive

**Solo / Collective / Insights.** The three privilege tiers
(`HELIXIR_MODE`). *Solo*: private, no cross-user visibility. *Collective*:
shared graph — cross-user dedup, consensus counting, `list_users`,
`swarm_status`. *Insights*: collective + the generative Moirai layer.
Higher tiers strictly add capabilities.

**Hive memory.** The collective-tier behaviour: one fact, many knowers.
When N agents state the same fact, it stays ONE memory node whose
`user_count` grows — consensus is a property of the node, not N copies.
Contradictions across agents are scored and surfaced, never merged away.

**Consensus / `user_count`.** The per-memory counter of independent
knowers, derived from `HAS_MEMORY` edges (idempotent per user — re-adding
doesn't inflate it). Ranking treats it as a consensus signal.

**Rendezvous.** Agents discovering each other *through the database
itself*, with no side channel: `add_memory(agent_id=...)` auto-heartbeats
the agent's presence (host, status, last-seen), and `swarm_status` returns
the live roster. (#39; `heartbeatAgent` in `schema/queries.hx`)

**Heartbeat.** The presence stamp behind rendezvous:
`register_or_heartbeat` upserts an `Agent` node with `last_seen`, host and
status. Fired implicitly by writes that carry `agent_id`.

**Confirm-or-promise.** The `add_memory` ack contract (#63). With the
ingest buffer on, the call *waits briefly* for the pipeline: if done in
time it returns the finished result (`ok:true` + `memory_ids`); if still
processing it returns `{ok:true, status:"accepted", pending_id}` — a
promise you can poll with `get_add_status`. Only `ok:false` is a failure;
`deduped` non-empty with `memories_added=0` means "already known", which is
success.

**Предбанник / ingest buffer.** ("Antechamber.") The bounded queue in
front of the write pipeline: `add_memory` enqueues and the pipeline drains
in the background, so a slow LLM never blocks the agent's turn.
Confirm-or-promise (above) is its ack contract. (`ingest.*` in config)

## Retrieval

**Read path.** The entire search machinery — dense vectors + BM25 + graph
expansion + PPR ranking — with **zero LLM calls**. `search_memory`,
`search_reasoning_chain`, `connect_memories` and `get_memory_graph` all run
on it; they work identically with no LLM configured at all.

**BM25.** The classic keyword-ranking function (term frequency × inverse
document frequency, length-normalized). Helixir uses HelixDB's native BM25
index as the *sparse* arm of hybrid search — it catches exact tokens
(names, ids, error codes) that embeddings blur.

**HNSW.** Hierarchical Navigable Small World — the approximate
nearest-neighbour index HelixDB uses for vectors. The *dense* arm of hybrid
search: it catches paraphrases and semantic neighbours that share no
keywords.

**RRF — Reciprocal Rank Fusion.** How the dense and sparse arms are
combined: each result contributes `1/(k + rank)` from each list it appears
in. Rank-based, so the two arms' incomparable scores never need calibrating.

**Ego-network.** The subgraph around the fused seed results — expanded one
batched HQL call per depth level across the edge families (relations,
entities, history, chunks...), keeping parent provenance. This is the
neighbourhood PPR runs over.

**PPR — Personalized PageRank.** PageRank with teleport biased to the seed
memories: a random walker keeps jumping back to what matched your query, so
centrality is measured *relative to the query*, not globally. Final rank =
`0.3·cosine + 0.5·PPR + 0.2·freshness`.

**Freshness.** The time component of ranking. Deliberately weak (0.2) and
attention-only: age affects what surfaces *first*, never what is
*reachable* — old facts stay in the graph and in chains forever.

**Provenance.** Every search result says where it came from:
`origin=seed|graph`, which edge pulled it in, from which parent, with what
PPR mass. Insights carry provenance too (witness memories per link). The
rule: a claim you can't trace is a claim you can't trust.

**Thin recall.** The hint `search_memory` appends when a personal-scope
search returns almost nothing: it suggests retrying with
`scope="collective"` before concluding the memory is empty — the store is
shared, and your identity may simply be new.

**Time window.** An explicit event-time bound on `search_memory`
(`time_from`/`time_to`, RFC3339 or `YYYY-MM-DD`; either side may be open).
It hard-filters the *seeds* — the direct answers — on event time
(`valid_from`, else `created_at`). `temporal_days` is the one-sided
shorthand for the same thing.

**Superseded flag.** A search result with `superseded: true` has been
replaced by a newer memory (`superseded_by` names it). It ranks below its
successor (score × `retrieval.superseded_penalty`, default 0.6) but stays
fully reachable — history, honestly labelled, never hidden.

**Flashback.** A memory from *outside* the requested time window that graph
expansion pulled back in through an edge to an in-window result. Never
hidden, never disguised: flagged `flashback: true` with its `event_date`,
and capped by a separate allowance (`retrieval.flashback_max`, default 3)
so associations never crowd the period's own rows. The recall analogue of a
human flashback — thinking about last week surfaces last year, and you know
it's old.

**Charter precedent.** One settled contradiction review, remembered: an
episode memory under `user_id=helixir` tagged with the dispute's shape
(new-type / old-type / verdict), SUPPORTS-linked to both disputed
memories. "Why does this rule exist" walks to the evidence.

**Learned rule.** After `write.rule_propose_after` identical precedents
(default 3), `resolve_contradiction` returns a ready-to-adopt
`rule_proposal`. Adoption is an explicit verbatim `add_memory`
("Charter rule [shape]: …", stored without LLM rephrasing). Adopted rules
render in `memory://rules` BESIDE the constitution — which itself never
self-learns — and silence further proposals of their shape.

**NLI edge router.** A local cross-encoder (deberta) that types confident
SUPPORTS (bidirectional entailment) and CONTRADICTS (same-subject
contradiction) edges before the LLM is consulted — part of making the
write path cheap (#96). Neutral or unconfident pairs still go to the
model; lean builds without the `nli` feature route everything to the LLM.

**Polysemy guard.** Lachesis' defence against apophenia hubs (#91): a
routed thread truncates at a pivot category whose neighbours sit in
different communities (label propagation over the PMI adjacency) with no
direct link between them — one word bridging two unrelated senses
("benchmarking": finance vs software). Genuine cross-domain links pass.
(`lachesis.polysemy_guard`)

**Verification duty.** Atropos' answer to insight debt (#91): aging
hypotheses get an adversarial review against their witness memories —
promote (relabelled `VERIFIED`), retire (superseded by a retirement note,
auto-demoted in search), or keep. Witness-less hypotheses retire as
unverifiable past `atropos.verify_unverifiable_age_hours`.

## Write path

**Content key / content-addressed dedup.** A normalized hash of (content,
type) stamped on every memory. The exact-duplicate gate: the same fact
re-added hits the same key group atomically (HelixDB `BatchCondition`) and
increments consensus instead of creating a copy.
(`add_pipeline/store.rs::content_key`)

**Decision engine.** The write-time judge: for each extracted atom against
its semantic neighbours it decides `ADD / UPDATE / SUPERSEDE / NOOP` (plus
cross-user `LinkExisting` / `Contradict` in collective), constrained by the
charter and the same-subject gate. (`src/llm/decision/`)

**Same-subject gate.** The rule (#46) that destructive verdicts (UPDATE /
SUPERSEDE / CONTRADICT / DELETE) are only legal when both memories are
about the *same specific subject*. Mere keyword overlap → `ADD` plus a
`RELATES_TO` edge. Prevents "my cat is black" from superseding "my dog is
black".

**Edge arsenal.** The seven typed memory→memory relations: causal/logical
`IMPLIES`, `BECAUSE`, `CONTRADICTS`, `SUPPORTS` (what
`search_reasoning_chain` walks) and associative/structural `RELATES_TO`,
`PART_OF`, `IS_A` (relatedness without a causal claim). All persist as one
`MEMORY_RELATION` edge whose `relation_type` property names the type — new
types need no schema change. (`src/toolkit/mind_toolbox/reasoning/`)

**NLI — Natural Language Inference.** The local ONNX judge (entail /
contradict / neutral) used as the paraphrase-merge backstop: two memories
merge only if each entails the other — and divergent numbers fail
entailment, which is exactly the anti-gaslight property. Runs offline;
`helixir model download` fetches the weights.

**Supersede (never delete).** There is no delete tool by design. An
outdated fact gets a `SUPERSEDES` edge from its replacement and a
`valid_until` stamp; history stays reachable through `HAS_HISTORY` forever.
Time affects attention, not reachability.

**Ontology.** The 20-concept tree (`Thing → Attribute/Event/Entity/
Relation/State → ...`) seeded into the graph at boot. Every memory gets an
`INSTANCE_OF` edge to its type's concept node; the 8 user-facing types
(fact, preference, skill, goal, opinion, experience, achievement, action)
are the leaves `search_by_concept` filters on.

## Generative memory — the Moirai

**Moirai.** The three Fates: background agents that *generate* connections
instead of waiting for them to be written. Clotho spins the category layer,
Lachesis measures threads through it, Atropos cuts the survivors into
insights. One orchestrated pass, on demand (`helixir pipeline`) or on a
schedule (the daemon). (`src/agents/`)

**Clotho — the Spinner.** Tags every memory from a controlled,
self-growing category vocabulary: embedding-match against the dictionary;
on a miss it *mints* a fitting category via the LLM. Shared tags weave
distant memories into subsets — the substrate the other two work on.

**Category layer / third axis.** What Clotho's tags add over the flat
graph: besides explicit edges (axis 1) and vector similarity (axis 2), two
memories can now be close because they share a *category* — which is how a
weather report and a battery stock end up in the same thread.

**Lachesis — the Measurer.** Routes multi-hop threads *within* the
category subsets and gates them against apophenia (below): a coherence gate
plus PMI subset-overlap per hop, drilling every link down to the anchor
memories that witness it.

**PMI — Pointwise Mutual Information.** `ln(|A∩B|·N / (|A|·|B|))` over
category member sets: how much more often two categories co-occur on the
same memories than chance predicts. The weakest link's PMI is a thread's
coherence floor; Atropos ranks by `hops × min_PMI`.

**Apophenia gate.** The defence against seeing patterns in noise (the
human failure mode of conspiracy boards). A thick, everything-touching
category has huge member sets, so its PMI with everything collapses toward
zero — it gates itself out *by arithmetic*, not by a blocklist.

**Atropos — the Cutter.** Curates routed threads into ranked, deduplicated
**insights**: enforces the quality bar, drops threads subsumed by longer
ones, journals the survivors, and persists each as a first-class
hypothesis-memory under `user_id=helixir` with `SUPPORTS` edges from its
witnesses — closing the loop: generated knowledge flows back into the
memory it came from.

**Insight.** The Moirai's unit of output: a cross-domain *hypothesis with
provenance* (category path + witness memories per link) and a lifecycle
(`proposed → verified → refuted`). Always framed as requiring
verification — the charter, extended to generated knowledge.

**Witness.** An anchor memory that evidences one link of an insight's
chain. Witnesses are what make an insight checkable — and what the
`SUPPORTS` provenance edges point from.

**Cadence.** Per-Moira pass frequency in the daemon
(`--clotho-every / --insight-every / --merge-every / --reconcile-every`, or
`moira.daemon.*_every_passes` in config): tagging can run every pass while
expensive insight routing runs every Nth. `0` disables a stage.

**The guar chain.** The canonical example of why chains beat facts:
*Rajasthan weather → guar harvest → guar gum price → fracking costs → shale
stocks*. No single edge stores it; the Moirai exist to reconstruct chains
like it. Used as the end-to-end validation fixture.

**Hygieia.** The built-in health watchdog (the 2026-07-02 OOM incident,
made an organ). Detectors — DB liveness, container memory, insight flood,
orphaned daemons, non-persistent storage — feed a reaction ladder:
self-heal silently (pause a flooding insights stage, restart a dead DB
container when allowed), alert THROUGH the memory itself (`ops_alert`
notices in agents' outboxes + a recallable `ops-alert` memory), push to a
human via the `on_alert_cmd` shell hook, journal everything
(`helixir health`). Runs inside the Moirai daemon, standalone
(`helixir watch`), or as a login service (`helixir watch install`).

**Cache valve.** Hygieia's lie detector for container memory: docker
stats charges reclaimable page cache to the container, so the memory
detector first opens cgroup `memory.reclaim` and only pressure that
SURVIVES the shed counts as live heap (`watchdog.allow_cache_reclaim`).
Observed live: 2.58GiB "used" collapsing to ~400MiB of real heap.

**Supervised memory restart.** HelixDB's in-process retention tracks
write churn and can reach the container cap within a day of heavy agent
traffic. When the post-reclaim (live) number crosses
`watchdog.mem_restart_pct` and restarts are allowed, Hygieia restarts the
container itself — volume preserved, ~10s, journaled as
`heal/mem_restarted` — instead of letting the OOM killer strike
mid-write. Judge the container with `tools/memprobe.py`, never docker
stats.

**Config hot-reload.** kubectl-apply for the memory: edit
`~/.helixir/helixir.toml`, run `helixir config apply`, running MCP
servers and gateways rebuild their client from the re-read config and
swap it atomically (SIGHUP; in-flight requests finish on the old client,
a failed rebuild changes nothing). `config get/set/edit` complete the
surface; `set` edits by dotted path with comments preserved. Active
FastThink sessions keep their pre-reload handle; `daemon`/`watch`
restart to apply.

## Working memory

**FastThink.** The isolated reasoning scratchpad (`think_start → think_add
→ think_recall → think_conclude → think_commit`): an in-process thought
graph (petgraph) that pollutes nothing until an explicit commit persists
ONE coherent conclusion. `think_discard` throws the session away;
timed-out sessions auto-save as `[INCOMPLETE]` and are recoverable via
`search_incomplete_thoughts`.

## Infrastructure

**HelixDB.** The single storage engine — graph and vector index in one
database (LMDB-backed), queried in HQL. There is no relational DB, no
Redis, no filesystem state; everything Helixir persists lives here.

**HQL.** HelixDB's query language. Schema (`schema.hx`) and named queries
(`queries.hx`) are *compiled into the server* — deploying a query change
means `helix check` + `helix push` (an image rebuild on the same data
volume), not a hot reload.

**Snapshot lag.** The HelixDB read-visibility quirk: a write can be
durable but not yet visible to the next read (index snapshot not refreshed).
Correct handling is re-probe with backoff — treating first-read-empty as
"lost" is the classic false alarm. (Also the reason some e2e are flaky by
design.)

**Error-in-HTTP-200.** The HelixDB failure mode where a query aborts but
the HTTP layer still answers 200 with the error inside the body. Helixir's
client surfaces these as real errors (#53); any raw probe script must check
the body, not the status code.

**MCP — Model Context Protocol.** The interface agents speak to Helixir:
a stdio (or streamable-HTTP via the gateway) server exposing the 21 tools.
Any MCP client — Claude Code, Claude Desktop, Cursor, zeroclaw — connects
with a few lines of config; `helixir setup` writes them for you.

**Layered config.** Effective settings = built-in defaults ←
`~/.helixir/helixir.toml` ← environment variables, later layers winning.
Gotcha: one invalid field rejects the whole TOML layer, and enum values are
capitalized (`mode = "Insights"`).

**Fallback chain.** The LLM resilience strategy: on *any* primary-provider
error (outage, exhausted quota) the same prompt cascades down an ordered
chain — by default `deepseek → ollama`, i.e. smart remote → cheap remote →
local selfhost — and the primary is readopted on its first successful call.
Tiers missing credentials are skipped at boot with a warning, never a boot
failure; the write result's metadata carries the full error trail that led
to the answering tier. Configured by `llm_fallback_chain` (or
`HELIX_LLM_FALLBACK_CHAIN`); an empty chain disables fallback.

**Causal stitching (lachesis-stitch).** Lachesis's second duty: a bounded
background pass over one user's recent memories that proposes candidate
pairs by entity overlap, asks an LLM judge whether an EXPLICIT causal
relation holds (conservative by prompt: co-occurrence is not causation),
and persists survivors as `BECAUSE` edges tagged `lachesis-stitch` —
hypothesis-grade provenance per the apophenia guardrail. Capped per pass
(window / judged / persisted) and convergent: already-linked pairs are
skipped, so re-running adds nothing.

**Family collapse.** Search-time compaction: a raw source and its extracted
atoms (linked by atom→raw `PART_OF` edges at write time) never coexist in
one result window. The best-ranked family member is kept; the folded ids
ride its `metadata.collapsed` and stay reachable. Either direction is legal
and content-lossless: atoms fold into a kept raw (their text is inside it),
or the raw folds under a kept atom (sibling atoms — distinct facts — stay).

**Presence TTL.** Roster hygiene for the swarm: agents silent past
`swarm.presence_ttl_secs` (default 30 min — above the daemon pass interval,
so healthy daemons don't flap) are presumed gone and hidden from
`swarm_status`, reported as `hidden_stale`. The Agent node itself is never
deleted: it anchors `AGENT_CREATED` authorship provenance on every memory
the agent wrote.
