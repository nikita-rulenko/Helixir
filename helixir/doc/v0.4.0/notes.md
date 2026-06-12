# v0.4.0 — the elder brain release

> _Frozen at release. Supersedes the `v0.4.0-pre` snapshot._

The theme of this release: memory that reasons, locally. The read path
makes **zero LLM calls**, the write path is curated by a charter instead
of silent rewrites, and every answer carries its own provenance.

`algo_opt` is now the **default** profile; `HELIXIR_RETRIEVAL_PROFILE=legacy`
preserves v0.3.x behaviour bit-for-bit. See `UPGRADING.md` for the
migration path (BM25 enablement + schema redeploy required).

## Read path

- **Hybrid retrieval**: dense ANN + HelixDB-native `SearchBM25` fused by
  RRF (k=60); temporal cutoff pushed into HQL with Rust defence-in-depth.
- **Levelwise batched graph expansion**: one `getConnectionsLevelBatch`
  call per BFS level — O(depth) round-trips instead of O(visited nodes).
- **Persistent embedding cache** (`HELIXIR_EMBED_CACHE_PATH`) + corpus
  warmup: zero embedding HTTP calls when warm; cold start 357 → 80 ms.
- **PPR re-ranking** over the typed ego-network: relevance mass
  accumulates along coherent paths instead of decaying per hop.
  MRR 0.582 → 0.687 on the golden set; distant-but-connected facts
  stay reachable (the long-range-deduction requirement).
- **LLM-free reasoning chains**: true BFS + cosine hop selection; chain
  seeds widen `contextual → full` on mature corpora; `nodes[]` exposes
  the peer memory (GH#23).
- **Provenance on every result**: `origin=seed|graph`, `edge`, `parent`,
  `depth`, `ppr`, raw `cosine` in metadata — chains are verifiable, not
  taken on faith.
- **`connect_memories(A, B)`** — new tool: bidirectional path discovery
  between two concepts with edge types and cumulative confidence;
  `shared_seed=true` marks anchor overlap as distinct from a real path.
- **`graph_depth` is honored** (was silently ignored), clamped to 1..=4.

## Write path

- **Memory charter** (`memory-charter.md`): conflicts the engine may not
  resolve silently come back in `add_memory.needs_clarification` with a
  ready-to-ask question. Constitution C1–C5; preference/goal/opinion
  rewrites escalate even at high confidence.
- **The engine can no longer delete memories**: a DELETE verdict executes
  as SUPERSEDE with history — the public no-deletion contract is now
  enforced in code.
- **Batched decisions (W1)**: all gray-zone facts of an add are judged in
  ONE LLM call; deterministic gates (W2: exact-match, cosine ≥ 0.98 with
  protected types exempt, below-threshold ADD) resolve the rest.
  Blocking LLM calls per add: was 1+N, now ≤ 2.
- **Stale conclusions are superseded**: "same mutable question, later
  state → SUPERSEDE" — yesterday's "X is next" no longer survives
  beside today's "X is done".
- **Hive cognitive layers**: the `HAS_MEMORY` edge carries the user's
  stance (`asserts` / `confirms` / `disputes`) with zero extra LLM calls;
  collective results expose the stance distribution per fact.
- **Self-seed** (`HELIXIR_SELF_SEED=1`): Helixir writes 26 facts about
  its own principles, charter and operational gotchas under
  `user_id="helixir"` — versioned, idempotent.

## Verification

Three e2e suites over the real surfaces (library, MCP stdio read, MCP
stdio write) + the hive cross-user suite; read suites run with a
deliberately dead LLM key. Golden-set quality bars: hit@5 10/10,
MRR ≥ 0.5 (measured 0.687). Measured on the bench corpus: warm search
p50 15–30 ms, chains 45–90 ms, MCP transport overhead ~0.2 ms.

## Known gaps → next milestones

- **Ingest buffer ("предбанник", #25)**: writes still block the caller
  (3–8 s on Cerebras, 15–40 s on local models); the buffer gives instant
  ack, hides 14B-extractor latency, and closes the parallel-write dedup
  race by serializing. Bidirectional: an outward queue will carry
  memory's questions to agents.
- **Sleep consolidation (#33)**: cross-domain edges via shared entities,
  hypothesis (guess) lifecycle on `verified`/`certainty`, retroactive
  supersession, duplicate sweep. Chains today are only as deep as the
  writer's sparse edges.
- **Temporal redesign (#31, blocked by #20)**: soft decay as default
  attention, window ⊥ depth, bi-temporality (`valid_from` is still a
  literal `{{timestamp}}` placeholder).
- **Charter increment 2 (#34)**: blocking semantics, `memory://rules`
  resource, self-learning allow-always rules with precedent provenance.
- **Epistemic search filter (#30-local)**: `guesses|verified|all`.
- **Local extraction quality**: 1.5–4B models lose ~40% of facts
  (extractor benchmark); the local-quality path is a 14B-class model
  behind the ingest buffer.
- Long tail: #9 architecture debt, #10 config reachability, #11
  Cargo.lock policy, #12 schema types, #13 deploy hygiene, #15.
