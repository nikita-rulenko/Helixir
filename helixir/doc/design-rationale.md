# Design rationale & evolution

> _Reflects code as of `v0.3.1-fix` plus the in-progress `dev` branch.
> Last verified: 2026-05-12._

This file is the **why** companion to the rest of `doc/`:

- `architecture.md` answers _how is it organized?_
- `data-model.md` answers _what is persisted?_
- `dataflow.md` answers _how does data move through the pipelines?_
- `userflow.md` answers _which tool does an agent call when?_
- `test-design.md` answers _what is guarded vs. left to drift?_
- **`design-rationale.md` (this file)** answers _what is the project, where is
  it heading, and why are the major decisions the way they are?_

It is intentionally opinion-bearing about historical decisions, citing
release tags as evidence. It is not a roadmap; do not file work from it
without a corresponding GitHub issue.

---

## 1. What Helixir is

Helixir is a graph-based persistent memory layer for LLM agents. Specifically:

- **A typed knowledge graph**, not a vector store with metadata. The base
  atom is `Memory` — one fact — classified into one of eight ontology
  types, linked to entities, concepts, contexts, and other memories by
  ~24 named edge types.
- **A decision-engine on the write path**, not append-only. Every
  `add_memory` call passes each extracted fact through `LLMDecisionEngine`,
  which picks one of `ADD / UPDATE / SUPERSEDE / CONTRADICT / LINK_EXISTING
  / CROSS_CONTRADICT / NOOP / DELETE`. The engine decides whether the new
  fact is a duplicate, a refinement, a replacement, a contradiction, or a
  cross-user echo.
- **A shared knowledge graph across users**, not a per-user silo. A fact
  known by two users is stored once and linked to both via `HasMemory`
  edges; `user_count` tracks the count. Both users can search collectively,
  detect controversies, and benefit from each other's dedup.
- **A two-tier memory system**: long-term graph in HelixDB, plus an
  in-process FastThink scratchpad (`petgraph`) for ephemeral reasoning that
  never reaches HelixDB unless the agent calls `think_commit`.

What Helixir **is not**:

- Not a chat history store. It does not log raw conversations; it extracts
  atomic facts from them.
- Not a RAG retriever. Vector search is one of three signals in the read
  path (vector + BM25 + smart traversal), and the write path actively
  curates what is persisted.
- Not user-isolated. Memory is shared by default at the graph level;
  what a user "sees" is controlled by their `HasMemory` edges and by
  `scope` parameters, not by per-user partitioning.
- Not a fixed-schema RDF/OWL system. The ontology is small, deliberate,
  and code-owned, not extensible by users at runtime.

### 1.1 The elder brain (north star)

The goal Helixir converges on is an **elder brain**: a memory that remembers
everything and uses connection structure to see what isolated facts cannot
show. The canonical example: *Rajasthan weather → guar harvest → guar gum
price → fracking fluid costs → shale stock valuations* — five facts, each
mundane, whose chain is an insight. Design consequences, each enforceable:

- **No deletion.** There is no delete MCP tool by design; supersession keeps
  history (`HAS_HISTORY`, `valid_until`). Pruning "irrelevant" facts destroys
  the middle of chains nobody has asked about yet. (The decision engine's
  internal `DELETE` path is scheduled to become SUPERSEDE-only — issue #34.)
- **Time governs attention, not reachability.** Temporal windows and decay
  apply to search entry points; graph traversal pulls connected context from
  any era. A three-day window anchors *where you start*, not *what exists*.
- **Distance must not equal death.** Per-hop multiplicative score decay made
  nodes beyond ~3 hops unreachable; PPR replaced it — relevance mass flows
  along edges and *accumulates* over coherent paths.
- **Chains are hypotheses, not proofs.** Every result carries provenance
  (origin, edge, parent, ppr) and every connection a cumulative confidence,
  so the agent can verify a chain instead of believing it.
- **The write side is governed by a charter** (`memory-charter.md`):
  a constitution of conflicts the engine may never resolve silently
  (deleting, rewriting preferences/goals/opinions, contradictions) —
  they escalate to the agent as `needs_clarification` questions.

---

## 2. Evolution by release

Releases as evidence of the project's direction. Source:
`gh release view <tag>` for every tag plus root `README.md`.

| Tag | Date | Theme | Key additions / fixes |
|---|---|---|---|
| `Rust` (v0.1.0) | 2025-11-29 | Initial Rust port | `HelixirClient`, `ToolingManager`, `mind_toolbox`, MCP server, base node/edge schema, vector search. |
| `Think_fast` (v0.1.1 / v2.0 internal) | 2025-12-01 | Working memory + protocol | FastThink (7 MCP tools: `think_start/add/recall/conclude/commit/discard/status`). Cognitive Protocol (built-in recall triggers + importance filters). Cognitive Seeds (agent personality / role). Incomplete-thoughts recovery on timeout. Server split into `params.rs` / `prompts.rs`. `HELIX_*` env prefix. |
| v0.2.0 | 2026-03-23 | Knowledge-graph foundation | Hive Memory Layer (cross-user dedup, collective search, controversy detection). Ontology System (8 types, `search_by_concept`). 33 edge / 15 node schema (24 active + 9 reserved). SmartTraversalV2. Atomic-fact extraction. Reasoning edges (IMPLIES/BECAUSE/CONTRADICTS/SUPPORTS). Embedding batching. `install.sh` + Makefile. EventBus (6 event types). |
| v0.2.1 | 2026-03-24 | Hive correctness | Cross-user dedup was silently broken — `user_id` was not propagated into `SearchResult.metadata`, so Phase 2 always saw empty candidates. Fix in `phases.rs`. Decision prompt strengthened with explicit cross-user rules. Phase 2 widened from `contextual/5` to `full/10`. First Hive E2E test. |
| v0.2.2 | 2026-03-24 | Performance | `add_memory` median 34.7s → 12.0s (2.9×). Phase 2 cross-user LLM decision moved to `tokio::spawn`. New `search_for_dedup` (lightweight, skips `user_count` / controversy enrichment). Parallel enrichment via `futures::join_all`. |
| v0.2.3 | 2026-03-27 | Retrieval quality I | Hardcoded `score = 0.8` replaced with rank-based exponential decay (`0.95 × 0.92^rank`). Spread of returned scores rose from ≈0 to > 0.4 — search stopped being effectively random. Extraction prompt rebalanced ("fewer, richer memories" instead of over-atomization). |
| v0.3.0 | 2026-03-27 | Retrieval quality II | Real cosine similarity by **re-embedding candidates on the client** (HelixDB does not return the cosine score from `SearchV` — this is a documented HelixDB choice). `list_memories` MCP tool (full-scan). Raw source storage (`source="raw_input"`) for long messages that yield multiple atomic facts — preserves entity definitions, API endpoints, etc., that atomization would otherwise lose. |
| v0.3.1 | 2026-03-27 | Reasoning quality | `get_chain` traversal grew from 3 to 8 edge directions; reasoning-chain hit rate rose from ~40% to ~95%. New `deep` chain mode (BFS to depth 8). Silent `break` on DB error replaced with `warn + continue`. Extraction now retries on invalid JSON / zero memories and falls back to single-memory storage. Coherence guard: `is_coherent_memory` + `split_incoherent_memory` prevents `UPDATE` from merging contradictory clauses about distinct subjects. |
| v0.3.1-fix | 2026-03-27 | Relation pipeline | Three independent root causes for `relations_created: 0`: Cerebras response_format (`"json"` → `"json_object"`); `enrich_memory_relations` now runs for all decisions except `NOOP/DELETE` (previously only `ADD/SUPERSEDE`); extraction-relation index mapping switched from sequential to `HashMap<usize, String>` so non-ADD operations no longer shift indices. |
| (in `dev`) | 2026-05-12 | Audit-driven hardening | CI on push/PR (#5). Blanket `#![allow]` removed (#6). Embedding URL single-source (#7). Self-loop guard in reasoning (#16). `(id, content)` pair consistency in chain projection (#17). Edge deduplication in `get_memory_graph` (#18). `list_memories` empty-user graceful path (#19). Real fallback score in `search_by_concept` (#22). |

Three sustained quality vectors are visible across these releases:

1. **Write quality.** Atomic extraction → balanced atomization → coherence
   validation → relation pipeline correctness. Each release tightens what
   the long-term store accepts.
2. **Read quality.** Random-by-accident scoring → rank decay → true cosine
   → reasoning-chain coverage. Each release makes "what comes back" track
   what was asked.
3. **User-vs-shared knowledge separation.** Hive Memory as explicit layer →
   correctness fix → moving off the critical path. The shared-graph
   invariant is steadily made faster, more reliable, and more correct.

---

## 3. Design choices and why

This section explains the load-bearing decisions. Each entry has the same
shape: **what**, **how**, **why**, and what alternative was rejected.

### 3.1 Atomic-fact memory, not blob storage

- **What.** Every `add_memory` call extracts a list of atomic facts from
  the user message before persisting.
- **How.** `LlmExtractor::extract` returns `ExtractionResult { memories,
  entities, relations, context }`. Each `memory` is a self-contained fact.
  Long inputs additionally store the original text as a
  `source="raw_input"` Memory (v0.3.0) so atomization is not destructive.
- **Why.** Downstream operations — dedup, supersession, contradiction
  detection, reasoning-chain traversal — work fact-by-fact. A blob of
  free-form text has no surface for any of these. Atomization is the
  enabling primitive for everything else in the system.
- **Alternative rejected.** "Store the message, embed it, return similar
  messages" (the standard RAG shape). This was viable as a prototype; it
  cannot represent supersession or contradiction.

### 3.2 Eight static ontology types

- **What.** Every Memory is classified as exactly one of
  `fact / preference / skill / goal / opinion / experience / achievement /
  action`. The list is fixed in code and schema; it does not grow from
  user data.
- **How.** `OntologyManager::map_memory_to_concepts` runs the LLM-derived
  type through the leaf nodes of the `Attribute` / `Event` subtrees of
  the canonical `Thing` tree (see `data-model.md §4`) and writes
  `INSTANCE_OF` / `BELONGS_TO_CATEGORY` edges. `search_by_concept` filters
  by this label.
- **Why.** The point of an ontology is to make retrieval intent-shaped:
  "what does the user want" is a different query than "what is the user
  good at" or "what happened last week". Eight types are enough to
  distinguish these intents without forcing users (or the LLM) to
  navigate a hierarchy. **Letting agents extend the ontology** at runtime
  was deliberately rejected: it pollutes the type space with case-by-case
  variants and destroys the intent-shape property.
- **Reserved `IS_A` / `CONCEPT_RELATED_TO` edges.** These exist in the
  schema (and have HQL queries) for normalized representation of the
  internal concept graph and for explicitly authored horizontal links
  between the fixed concepts. They are **not** a runtime extension hook.

### 3.3 Decision matrix on every write

- **What.** Each atomic fact is processed by `LLMDecisionEngine::decide`,
  which returns one of `ADD / UPDATE / SUPERSEDE / CONTRADICT /
  LINK_EXISTING / CROSS_CONTRADICT / NOOP / DELETE`. The pipeline acts on
  this decision; the user does not pick.
- **How.** Phase 1 searches the caller's memories (`scope=personal`,
  contextual mode). Phase 2 searches across all users
  (`scope=collective`, full mode, in the background since v0.2.2). Both
  result sets feed the decision prompt; the engine picks the operation.
- **Why.** A memory layer for an agent must answer not just "is this
  similar?" but "**what should I do** with it?" Append-only stores grow
  forever and dilute relevance. Time-windowed stores forget too much.
  A typed decision per fact lets the system both keep the store compact
  and preserve the trail (`SUPERSEDES`, `CONTRADICTS`, `HAS_HISTORY`).
- **Alternative rejected.** "Always append, dedupe at query time." This
  pushes the cost of curation to every read, makes contradiction handling
  impossible, and never converges.

### 3.4 Shared memory across users (Hive Memory)

- **What.** A fact is stored once as a `Memory` node. Every user who
  knows it is linked to that single node by a `User -[HasMemory]-> Memory`
  edge; `user_count` tracks the count.
- **How.** Phase 2 of `add_memory` searches `scope=collective`. If a
  matching fact already exists for another user, the decision engine emits
  `LINK_EXISTING` and the background fan-out (`link_user_to_memory_bg`)
  adds the new `HasMemory` edge and bumps `user_count`. Cross-user
  contradictions are wired through `CROSS_CONTRADICT`.
- **Why.** Memory layers built per-user silo can only ever recall what a
  given user said. A shared knowledge graph can answer "what does anyone
  here know about X?" and "who disagrees about Y?" with the same primitives
  it already uses for the single-user case. Privacy semantics live at the
  `scope` parameter and at what the agent chooses to ingest, not at the
  data layer.
- **Alternative rejected.** Per-user isolation. Considered and rejected
  for v0.2.0 — see `architecture.md §4 "Shared memory across users"` for
  the invariant statement, and issue #21 (closed `not planned`) for the
  case where this design was mistaken for a privacy leak by an auditor.

### 3.5 Reified justifications: `BECAUSE / IMPLIES / SUPPORTS / CONTRADICTS`

- **What.** Reasoning is a first-class part of the graph: four Memory →
  Memory edge types capture causal and logical relationships. Plus
  `MEMORY_RELATION` as a generic typed edge.
- **How.** Inferred by the LLM during the enrich phase of `add_memory`
  (since v0.3.1-fix this runs for all decisions except `NOOP/DELETE`).
  Retrieved by `search_reasoning_chain`, which BFS-traverses both
  directions of every reasoning edge (since v0.3.1 — coverage rose from
  ~40% to ~95% with the move from 3 to 8 traversed directions).
- **Why.** "Why do you think X?" is a question agents answer constantly.
  Without reified justifications, the answer has to be re-derived from
  vector-similar text on every call. With explicit edges, the answer is
  one graph walk away and is stable across sessions.
- **Alternative rejected.** Encoding justifications as text in
  `Memory.metadata`. Rejected because it is invisible to graph traversal
  and impossible to use for follow-up queries like "what else does X
  contradict?".

### 3.6 FastThink as a separate subsystem

- **What.** A `petgraph::stable_graph::StableDiGraph<Thought, Edge>` held
  in memory per session. Seven MCP tools (`think_start/add/recall/
  conclude/commit/discard/status`). Nothing reaches HelixDB until
  `think_commit`.
- **How.** `FastThinkManager` keeps a `HashMap<session_id, ThinkingSession>`
  under `RwLock`. Default limits: 90 s wall clock, 150 thoughts. On
  timeout, `commit_partial` writes a Memory tagged
  `context_tags=incomplete_thought` so the session can be picked up later
  via `search_incomplete_thoughts`.
- **Why.** Long-term memory is expensive to write to (extraction,
  embedding, decision, enrichment) and expensive to read from later
  (every hypothesis pollutes future searches). FastThink lets the agent
  reason without committing — branching, recalling existing facts in,
  contradicting itself — and only commit the conclusion. The shape mirrors
  human working memory: scratch first, decide later.
- **Alternative rejected.** Treating intermediate thoughts as regular
  memories. Rejected because every false start would dilute the
  long-term store and the decision engine would have to reason about its
  own scratchwork.

### 3.7 Real cosine re-ranking on the client

- **What.** Search results are re-embedded on the client and ranked by
  actual `cosine_similarity(query, candidate)`, not by a placeholder
  score from HelixDB.
- **How.** `SmartTraversalV2` calls `EmbeddingGenerator::generate` for
  each candidate and computes the cosine in
  `mind_toolbox/search/smart_traversal/scoring.rs`. The embedding cache
  (`moka`, LRU 1000, TTL 300 s) keeps re-embedding cheap for repeated
  queries.
- **Why.** HelixDB's `SearchV` returns a result-set ordered by similarity
  but does not serialize the cosine distance in JSON (the `HVector`
  `Serialize` impl excludes it by design). Earlier Helixir releases used
  a hardcoded `0.8`, then a rank-based exponential decay — both gave
  rank-correct but query-independent scores. Re-embedding on the client
  is the documented workaround and makes scores compare across queries.
- **Cost / benefit.** One extra embedding call per candidate per search,
  amortized by the cache. Acceptable in exchange for scores that mean
  something.

### 3.8 Coherence guard on `UPDATE`

- **What.** If `LLMDecisionEngine` returns `UPDATE` with a merged content
  that mixes contradictory clauses about distinct subjects, the operation
  downgrades to `ADD` and the existing memory is preserved untouched.
- **How.** `is_coherent_memory` runs first-pass detection of
  contradiction markers (`but`, `however`, `although`, …) across multiple
  subjects within a candidate. `split_incoherent_memory` splits at the
  markers; the decision prompt is also instructed to refuse incoherent
  merges (`src/llm/decision/prompt.rs`).
- **Why.** Without this guard, an `UPDATE` could turn "I love coffee" +
  "I quit coffee" into a single memory that says "I love coffee but I
  quit coffee" — which is not a fact, it is a story. The guard lets the
  system pick the side it can defend (or store both as `CONTRADICTS`)
  instead of fabricating a Frankenstein.

### 3.9 Raw-source preservation alongside extraction

- **What.** For inputs longer than 100 chars that yield more than one
  extracted fact, the original message is also stored as a
  `source="raw_input"` Memory.
- **How.** Conditional save in `store_raw_source`
  (`add_pipeline.rs:932`). It is tagged `memory_type="fact"` and
  participates in normal search.
- **Why.** Atomization is lossy by design — it strips API endpoint
  signatures, entity field lists, code snippets, dependency chains. The
  raw-source backup means the agent can still recover the literal form
  when atomized facts are not specific enough.

### 3.10 Per-write decision cost is on the writer, not the reader

- **What.** All decision and enrichment work happens during
  `add_memory`, not during `search_memory`. The read path is fast-by-design.
- **How.** The two-phase add pipeline pays the extraction-decision-enrich
  cost up front. The read pipeline only does vector + BM25 + traversal
  with cached embeddings.
- **Why.** Agents call search many more times than add. Putting the
  costly LLM work on the writer lets the reader stay sub-second even
  when the corpus is large.
- **Concession.** This makes `add_memory` slow — 34.7 s before v0.2.2,
  12 s after. The trade-off is explicit and documented in the v0.2.2
  release.

---

## 4. Capability surface (one-table reference)

For an exhaustive list of what the system provides today, see
`architecture.md §7 "Capability surface"`. The short version:

```
write:                      add_memory
read (semantic):            search_memory  (modes: recent/contextual/deep/full)
                            search_by_concept (8 types)
                            search_reasoning_chain (4 reasoning edges, BFS)
read (exhaustive):          list_memories
read (graph view):          get_memory_graph
read (recovery):            search_incomplete_thoughts
mutate:                     update_memory
working memory (FastThink): think_start / think_add / think_recall /
                            think_conclude / think_commit / think_discard /
                            think_status
collective layer:           scope = personal | collective | all
                            user_count, controversy detection,
                            CROSS_CONTRADICT
audit & history:            HAS_HISTORY edges, HistoryEvent nodes
versioning:                 SUPERSEDES edges
```

---

## 5. Direction (read off the reserved schema surface)

The schema declares nodes/edges and HQL queries for surfaces that the
Rust pipeline does not yet wire. Each reserved surface is a design
decision already made:

- **`DocPage / DocChunk / CodeExample / ErrorCode`** + their edges
  (`PAGE_TO_CHUNK`, `CHUNK_TO_EMBEDDING`, `CHUNK_MENTIONS_CONCEPT`,
  `CONCEPT_HAS_EXAMPLE`, `ERROR_REFERENCES_CONCEPT`) — memory of
  **documents and codebases**, not only conversations.
- **`Constraint` + `APPLIES_IN`** — rules that hold within a context
  ("at work I don't drink coffee"); enables per-context filtering and
  policy.
- **`Session` + `IN_SESSION` / `CREATED_IN`** — conversation-scope
  views of memory; time-windowed reasoning over a session's contributions.
- **`PART_OF`** — entity composition, enabling structural queries
  ("which engines belong to which cars").
- **`IS_A` / `CONCEPT_RELATED_TO`** — normalized internal concept-graph
  edges; **not** runtime ontology extension (the 8 types stay fixed).

These are not promises of when they will ship — they are the architectural
intent already baked into the schema.

---

## 6. How to use this document

- When designing a feature: read §3 first to see whether your proposal
  is consistent with the load-bearing decisions or breaks one.
- When auditing a "bug": cross-reference with §3 — what looks like a bug
  from a general-purpose engineering lens (e.g. "list_memories returns
  data of other users") may be the explicit intent (§3.4). The audit
  failure modes are catalogued in `AGENTS.md §11 "Helixir-specific
  tripwires"`.
- When writing release notes: every release should map to at least one
  vector in §2 — if it does not, it likely belongs in the architectural
  backlog instead of a customer-facing release.
- When opening an issue: if your title implies a load-bearing decision is
  wrong, cite the relevant §3 entry and explain why the trade-off no
  longer applies. Do not silently re-litigate.
