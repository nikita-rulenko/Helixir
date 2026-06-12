# Graph traversal & retrieval research — proposals for acceleration

> **Status (2026-06-12):** this research is largely SHIPPED on `dev` under the
> `algo_opt` retrieval profile. Landed: P0.1–P0.4, P1.1 (native BM25 + RRF),
> P1.2-equivalent (LLM-free embedding-guided chains), P1.3 (levelwise batched
> expansion), P2.3, plus PPR re-ranking and `connect_memories` that grew out
> of §6. Open: P1.4 (HNSW tuning), P1.5a (HelixDB distance exposure), P2.1,
> P2.2, P2.4. Kept as the historical record of the reasoning behind the work.

> _Reflects code as of `dev` @ e1b05e5. Last verified: 2026-05-12. Source for §1–§3 is the audit performed on the live build; §4–§6 cross-reference HelixDB docs and current RAG literature (2026)._

This document collects everything we learned from a focused research pass on
graph traversal theory, HelixDB's native primitives, and the current state of
Helixir's retrieval code. It ends with prioritized proposals for accelerating
graph walking and memory search.

It is **research output**, not a roadmap. Each proposal links back to the
finding that motivates it and the load-bearing invariant it must respect
(`design-rationale.md` §3). Implementation tickets are out of scope here —
file them separately when we agree on the direction.

---

## 1. Executive summary

- **The live retrieval engine is `SmartTraversalV2`.** Everything else in
  `search/` — `HybridSearch`, `Bm25Search`, `QueryProcessor`,
  `MemoryChainStrategy`, `RetrievalManager` — is **declared but never
  instantiated** from the MCP path. This is in addition to the dead twins
  we already excluded from the build (`integrator/`, `onto_search/`).
- **Helixir re-implements on the client what HelixDB already provides
  natively.** Native primitives we do not call today: `SearchBM25`,
  `ShortestPathBFS<E>`, `ShortestPathDijkstras<E>`, post-filter with
  `SearchV(...)::WHERE`, multi-hop chain `::Out<E>::Out<E>::RANGE(...)`,
  HNSW tuning via `helix.toml`. See §3.
- **Two correctness smells in the current pipeline.** Phase-2 graph
  expansion assigns a hard-coded `semantic_sim = 0.5` to every graph
  neighbour (`smart_traversal_v2/phases.rs:407`); the search cache key
  excludes `temporal_cutoff` (`traversal.rs:177-203`). Both bias results
  in non-obvious ways. See §6 P0.
- **The reasoning chain isn't a BFS despite its name.** It pops from a
  `Vec` (LIFO), so it's a DFS-with-frontier, and in non-`deep` modes it
  asks the LLM to pick **one** next neighbour per step (`reasoning/chain.rs`).
  Bidirectional BFS or a beam over native `ShortestPathBFS` would give
  predictable order and better recall. See §6 P1.
- **No hybrid retrieval today.** Dense vector + graph only. Industry
  baseline since 2023 (re-confirmed in 2026 literature) is dense + BM25
  fused via Reciprocal Rank Fusion (RRF, k=60). HelixDB supports
  `SearchBM25` natively, so the cost to add this is small. See §6 P1.

---

## 2. Current state — what runs on the live MCP path

### 2.1 Active pipelines (entry points)

| MCP tool | Entry function | Backend chain |
|---|---|---|
| `search_memory` | `ToolingManager::search_memory` → `SearchEngine::search` | `SmartTraversalV2` (mode `recent/contextual/deep/full` → depth 1/2/3/4); fallback `VectorSearch::search` if traversal not enabled |
| `search_by_concept` | `ToolingManager::search_by_concept` | `SearchEngine::search` seed (×3 limit) → per-candidate `getMemoryConcepts`; fallback to `getUserMemories` + offline scoring |
| `search_reasoning_chain` | `ToolingManager::search_reasoning_chain` | `SearchEngine::search` (contextual seeds) → `ReasoningEngine::get_chain` (frontier-based traversal with `Vec::pop()`) |
| `get_memory_graph` | `ToolingManager::get_memory_graph` | per-depth loop: `getMemory` + `getMemoryLogicalConnections` |
| `add_memory` Phase-1 recall | `add_pipeline/orchestrate.rs::add_memory` | `SearchEngine::search` (contextual, personal, k=5) |
| `add_memory` Phase-2 cross-user | `add_pipeline/cross_user.rs` | `SearchEngine::search_for_dedup` (effective_user_id = None) |
| `think_recall` | `FastThinkManager::recall` → `HelixirClient::search` | Same as MCP `search_memory`, contextual mode, k from `FastThinkLimits::max_recall_results` (8 for MCP) |

### 2.2 `SmartTraversalV2` — what the live engine actually does

Three phases, all visible in `mind_toolbox/search/smart_traversal_v2/`:

1. **Vector phase** (`phases.rs::vector_search_phase`).
   - Calls HQL `smartVectorSearchWithChunks` with `query_vector` and
     `fetch_limit = top_k * 3` (if scope is personal).
   - HelixDB does not surface its real similarity through edge
     traversal (`HVector.distance` is intentionally not in JSON), so the
     code approximates the score with **exponential rank decay**:
     `score = 0.95 * 0.92^rank` (`phases.rs:107-130`).
   - **Personal scope** filters `memory.user_id == uid` in Rust after
     ANN returns (`phases.rs:114-122`).
   - **Temporal cutoff** filters `memory.created_at >= cutoff` in Rust
     (`phases.rs:124-135`).
2. **Client-side cosine rerank** (`traversal.rs:74-110`).
   - For accepted hits, batch re-embed their `content` and compute
     `cosine_score(query, content)`.
   - If `|real_score - rank_proxy_score| > 0.01`, overwrite the score
     and re-sort. Hence the cosine number you see in MCP output is the
     **real cosine after re-embedding**, not HelixDB's HNSW score.
3. **Graph expansion** (`phases.rs::graph_expansion_phase`).
   - Per accepted vector hit, spawn a `tokio::task`. Each task does a
     depth-limited DFS using `getMemoryLogicalConnections` once per
     visited node, walking 8 edge families (out/in × IMPLIES, BECAUSE,
     CONTRADICTS, MEMORY_RELATION) with per-family weights.
   - At each depth level, neighbours are sorted by weight and the top 3
     are kept (`take(3)`).
   - **Every graph node gets `semantic_sim = 0.5` hard-coded**
     (`phases.rs:407-410`). The `query_embedding` parameter is passed
     through but never used inside `expand_from_node`.
4. **Rank & filter** (`phases.rs::rank_and_filter`) — merge duplicates
   by max `combined_score`, drop below `min_combined_score`, sort.

### 2.3 HelixDB queries actually called by the live retrieval path

```
smartVectorSearchWithChunks        # ANN backend
getMemoryLogicalConnections        # neighbour fetch (per node, per hop)
vectorSearch                       # fallback ANN
getMemoryUsers                     # collective enrichment
getMemoryContradictions            # collective enrichment
getMemoryConcepts                  # per-candidate in search_by_concept
getUserMemories                    # list / fallback in search_by_concept
getMemory / getMemoryWithChunks    # graph + chunk reconstruction
getMemoryReasoningRelations        # context assembler
getMemoryEntities                  # context assembler
getRecentContexts / getContext     # context manager
```

### 2.4 Dead retrieval infrastructure still on disk

These compile but are not on any live call chain (no caller outside the
module). Listed for completeness; recommendation in §6.

| Module | Purpose stated by comments / structure | Lines | Status |
|---|---|---|---|
| `search/hybrid.rs` (`HybridSearch`) | vector + BM25 with weights | ~180 | dead |
| `search/bm25.rs` (`Bm25Search`) | client-side BM25 TF-IDF on candidates | ~170 | dead |
| `search/query_processor/` | query enhancement / intent | ~330 | dead |
| `mind_toolbox/memory_chain/` (`MemoryChainStrategy`) | vector seeds + recursive chain expansion | ~520 | dead |
| `mind_toolbox/memory/retrieval.rs::RetrievalManager` | wrapper around retrieval queries | ~110 (parts) | dead in the sense of `RetrievalManager` never constructed; the file has live helpers used elsewhere |

---

## 3. What HelixDB actually offers (and we don't use today)

Sourced from the official docs (`docs.helix-db.com`, llms.txt mirror via
context7, GitHub README) on 2026-05-12.

### 3.1 Vector

- `SearchV<Document>(vector, limit)` — HNSW ANN; **supports inline
  `::WHERE` post-filter** (`docs.helix-db.com/.../searching` example 2:
  `SearchRecentDocuments`). Today Helixir filters in Rust *after* the
  call — we lose pre-filter speedups.
- `Embed(text)` — server-side embedding. Not relevant for Helixir
  (provider is configured client-side), but worth noting.
- HNSW parameters are tunable in `helix.toml`:

  ```toml
  [local.dev.vector_config]
  m                = 16
  ef_construction  = 128
  ef_search        = 768
  db_max_size_gb   = 20
  ```

  `ef_search` is the single biggest recall/latency knob and is **not
  currently audited** for Helixir's profile.

### 3.2 Keyword search (BM25)

```hx
QUERY SearchKeyword (keywords: String, limit: I64) =>
    documents <- SearchBM25(keywords, limit)
    RETURN documents
```

- Enabled per-instance with `bm25 = true` in `helix.toml`.
- Real BM25 over node text properties, with HelixDB's index. Helixir's
  own `bm25.rs` is a client-side TF-IDF on already-retrieved
  candidates — strictly weaker and not on the live path.

### 3.3 Graph traversal — built-in shortest path

```hx
QUERY RouteHops (from_id: ID, to_id: ID) =>
    path <- N<City>(from_id)::ShortestPathBFS<Road>::To(to_id)
    RETURN path

QUERY RouteDistance (from_id: ID, to_id: ID) =>
    path <- N<City>(from_id)::ShortestPathDijkstras<Road>(_::{distance_km})::To(to_id)
    RETURN path
```

- `ShortestPathBFS<EdgeType>` — fewest hops.
- `ShortestPathDijkstras<EdgeType>(weight_property)` — weighted.
- Multi-hop chains in HQL itself: `::Out<E>::Out<E>::RANGE(0, 40)` —
  one server roundtrip per chain, no N+1.

Today the reasoning chain (`reasoning/chain.rs`) and the search-side
graph expansion (`phases.rs`) both do **one HelixDB call per visited
node**.

### 3.4 Schema-side filtering and indexes

- `WHERE`, `EXISTS`, scalar comparisons (`EQ/NEQ/GT/GTE/LT/LTE`) inline.
- Secondary indexes and unique indexes (`schema/secondary-indexing`).
  Worth checking which Memory fields would benefit from indexing
  (`user_id`, `memory_type`, `context_id`, `created_at`?). The schema
  audit in `data-model.md` §4 is the right place to update once a
  decision is made — current schema doesn't declare extra indexes.

---

## 4. Graph theory primer — what applies and where

Quick map of the algorithms relevant to memory retrieval / reasoning
traversal, with their best use in Helixir.

| Algorithm | Complexity | Best for in Helixir | Where applicable |
|---|---|---|---|
| BFS (single-source) | O(V+E) per source | Hop-bounded neighbourhood from a seed | Graph expansion in `SmartTraversalV2` Phase 2; chain forward/backward walk from a seed |
| Bidirectional BFS | O(b^(d/2)) instead of O(b^d) | Path query between two known endpoints | "Why is A linked to B?" / `get_chain(a, b)`; reasoning chain when both seed and target are known |
| Beam search (top-K BFS) | O(B·d) where B = beam width | Bounded-recall traversal in dense graphs | Replacing the `take(3)` hard cut in Phase 2 with a top-B by global score frontier |
| Dijkstra | O((V+E) log V) | Cheapest weighted path | If edge weights matter (e.g. confidence, certainty) — already native in HelixDB |
| A\* | O(b^d) but heuristic-pruned | Embeddings give a natural admissible heuristic | Reasoning chain when we have a target embedding; use `cosine_score(node, target)` as `h` |
| Multi-source BFS | O(V+E) once across all sources | "How is any of these seeds connected to X" | Aggregating multiple top vector hits into a single graph expansion in Phase 2 |
| Random Walk with Restart | O(k·E) | Personalization / proximity to a seed cluster | Score boosting around a "current focus" memory (e.g. last referenced) |
| Page-rank-style centrality | O(k·E) preprocess | Identifying hubs in the knowledge graph | Out-of-band offline job; could boost or down-weight "popular" memories |

The expensive thing in our domain is the **per-node HelixDB call**, not
the in-process algorithm. Anything that **moves the traversal server-side**
or **reduces the number of nodes visited** is a win.

---

## 5. Modern retrieval best practice (RAG, 2026)

Distilled from the recent material we pulled (Tavily search 2026-05-12).

1. **Hybrid retrieval is the new baseline.** Dense embeddings + BM25
   fused via RRF (Reciprocal Rank Fusion, `1/(k+rank)`, conventional
   `k=60`) consistently beats either signal alone. Score-agnostic, no
   normalization needed. Industry standard since SIGIR 2009; re-affirmed
   by Vertex AI, Qdrant, Weaviate, Redis docs in 2026.
2. **Two-stage retrieval is mandatory at scale.** Stage 1 — recall (ANN
   over HNSW or IVF-PQ, large `k`). Stage 2 — precision (cross-encoder
   reranker on top-50, e.g. `bge-reranker-v2-m3`). The reranker is "the
   single biggest precision gain" per all 2026 surveys.
3. **HNSW dominates production.** Logarithmic search complexity, M=16
   and `ef_construction=128–200`, `ef_search` is tuned per workload
   (recall@k target vs. latency SLO). At the scale we're at (<10M
   memories), HNSW alone is enough; IVF-PQ is for >100M.
4. **Reranking re-embedding (what we do today) is between Stage 1 and
   Stage 2.** It's cheaper than a cross-encoder, but it doesn't fix
   recall — if the right document isn't in the ANN top-K, no re-rank
   recovers it. We need to lift recall first.
5. **Bidirectional BFS is the standard trick** for path queries on a
   graph with branching factor `b` and distance `d`: O(b^(d/2)). Common
   pitfall is alternating one **node** per side instead of one **level**
   per side — needs to be one level. The other common refinement is to
   advance whichever frontier is smaller, ~100× speedup on long paths
   (per the `zdimension.fr` write-up, replicated by Wikipedia).
6. **MMR (Max Marginal Relevance)** for result-set diversification is
   reliable and cheap. Set `λ=0.5` to balance relevance vs novelty;
   applies on the **final top-K** before sending to the LLM.
7. **Late-interaction (ColBERT/MaxSim)** is a precision lever for very
   long documents. Helixir stores **atomic facts**, so the per-token
   late-interaction win is small relative to its storage cost. Not
   recommended for us.

---

## 6. Proposals (prioritized)

Severity rubric follows `AGENTS.md` §4.4. Each item names the finding
that motivates it, the load-bearing invariant it must not violate, and a
qualitative estimate of impact.

### P0 — correctness / cheap wins

#### P0.1 Push `temporal_cutoff` into HQL `::WHERE`

- **Finding.** `phases.rs:124-135` filters `memory.created_at >= cutoff`
  in Rust *after* HelixDB returns. This means every ANN result is paid
  for, even those that will be dropped.
- **Proposal.** Add a `getMemoryWithinWindow` HQL query that uses
  `SearchV<Memory>(vector, limit)::WHERE(_::{created_at}::GTE(cutoff))`
  (or update `smartVectorSearchWithChunks` to accept an optional
  cutoff). Move the Rust filter behind it as defence in depth.
- **Impact.** Linear in (`fetch_limit` − `top_k`). For `recent` mode the
  current `fetch_limit = top_k * 3` is non-trivial. Larger expected win
  for `recent` (4h window) than `full` (no window).
- **Invariant.** `design-rationale.md` §3.7 (writer pays cost, reader
  stays fast) — pre-filter strictly cheapens the reader.

#### P0.2 Replace `semantic_sim = 0.5` hard-coded for graph nodes

- **Finding.** `phases.rs:407-410`. Every node reached via graph
  expansion is treated as if it had cosine similarity 0.5 to the query.
  This silently flattens ranking and pushes graph-reached nodes into a
  fixed score band.
- **Proposal.** Use the real `cosine_score(query_embedding, node_emb)`
  for graph nodes too. The neighbour content is already on the wire
  (`memory.content`) — re-embed and score with the same logic as Phase 1
  rerank (`traversal.rs:94-111`). Alternative if re-embedding cost is a
  concern: inherit the parent's vector score weighted by edge weight,
  i.e. `propagated_sim = parent_vector_score * edge_weight`.
- **Impact.** Direct ranking accuracy on every graph-expanded result.
  Combined-score correlations with relevance will rise.
- **Invariant.** §3.6 (real cosine re-rank is *the* signal we trust
  client-side; extend it consistently).

#### P0.3 Include `temporal_cutoff` in `SmartTraversalV2` cache key

- **Finding.** `traversal.rs::make_cache_key:180-205` excludes the
  cutoff. Two queries that share embedding/user/config but have
  different time windows can collide on the same cached entry.
- **Proposal.** Add `temporal_cutoff` (or `effective_temporal_days`) to
  the hashed key. Also add the `mode` string so `contextual` and `deep`
  don't share cache entries when other config bits happen to overlap.
- **Impact.** Cache correctness, not throughput. Today's failure mode
  is silently returning stale or out-of-window memories on cache hit.

#### P0.4 Apply `cache_ttl` to LRU `put`

- **Finding.** `traversal.rs:159-161` calls `cache.put` without TTL
  even though `cache_ttl` is stored on the struct (and currently flagged
  `#[allow(dead_code)]`).
- **Proposal.** Wire the TTL through the LRU layer. Right now there is
  no automatic expiry, so a Memory updated by `update_memory` won't be
  reflected in cached search results until the entry is evicted.

### P1 — architectural wins

#### P1.1 Native hybrid retrieval (dense + BM25, fused with RRF)

- **Finding.** No keyword-search signal on the live path. Class of
  failure: dense embeddings miss exact identifiers, error codes, names —
  these are exactly the things users search for in agent contexts ("the
  function `cosine_score`", "issue #25", "Cerebras gpt-oss-120b").
- **Proposal.**
  1. Enable `bm25 = true` in HelixDB instance (cheap; instance-level
     toggle).
  2. Add HQL `searchMemoryByKeywords(keywords: String, limit: I64) =>
     mems <- SearchBM25(keywords, limit) RETURN mems`.
  3. In `SearchEngine::search`, run vector + BM25 in `tokio::join!`,
     then fuse with RRF (`score(d) = sum 1/(k + rank_i(d))`, `k=60`).
     Result: a single top-K list scored by rank consensus. **Do not
     normalize cosine and BM25 scores numerically — RRF doesn't need it.**
  4. Keep the existing rerank step on top of the fused top-K; cross-
     encoder rerankers can come later.
- **Impact.** Recall@K on identifier-heavy queries jumps substantially
  in every public benchmark (Vertex AI, LanceDB, Qdrant, Weaviate
  numbers). The current `bm25.rs` and `hybrid.rs` files become live
  again under this work (or are replaced wholesale).
- **Invariant.** §3.1 (shared graph). BM25 indexes Memory `content`,
  which is shared; the existing `HasMemory` user link continues to
  separate knowers from facts.

#### P1.2 Move reasoning chain onto native `ShortestPathBFS`

- **Finding.** `reasoning/chain.rs` does N HelixDB calls (one
  `getMemoryLogicalConnections` per visited node). For depth-8 traversal
  this is dozens of round-trips. The frontier is a `Vec::pop()` (LIFO,
  i.e. DFS), comment claims BFS — so behaviour is unpredictable.
- **Proposal.** For path queries (`get_chain(from, to)`):
  - Use `ShortestPathBFS<E>::To(to_id)` per edge family, then merge.
  - For "deep" mode (no target), keep client-side BFS, but **switch to a
    real BFS** (replace `Vec` with `VecDeque`) and use a beam of width
    B = `chain_beam_width` (config field) per level instead of asking
    the LLM for one neighbour at a time.
- **Impact.** Round-trip count drops from O(d·b) to O(d) on path
  queries; behaviour matches the comment. For "deep", recall improves
  because beam keeps more candidates alive.
- **Invariant.** §3.3 (reasoning edges are first-class). Native BFS
  walks them at server speed.

#### P1.3 Multi-hop chain in HQL for graph expansion

- **Finding.** `phases.rs::graph_expansion_phase` does
  `getMemoryLogicalConnections` once per node, recursively. For depth 4
  with branching ~3 that's up to ~80 round-trips per query.
- **Proposal.** Author a single HQL `expandMemoryGraph(seed_ids,
  max_depth)` that returns the depth-bounded neighbourhood directly via
  `::Out<E>::Out<E>::RANGE(0, 40)` chains for each edge family,
  flattened into a `(memory_id, depth, edge_path)` projection.
- **Impact.** Round-trips drop by a factor of (`b^d`) per seed. Tail
  latency on `mode=deep` / `mode=full` will shrink markedly.
- **Invariant.** §3.6 (writer-pays-cost). This shifts work to the DB,
  which is already the right place.

#### P1.4 Tune HNSW `ef_search` for our access pattern

- **Finding.** `helix.toml` is not part of the Helixir repo (we run a
  vendored HelixDB instance); we don't have evidence that `ef_search`
  was ever calibrated for our `k=5–50` workload and ~1k–100k memories.
- **Proposal.** Run a one-off recall@10 benchmark sweep at `ef_search`
  ∈ {64, 128, 256, 512, 768} on a representative corpus + held-out
  query set. Pick the smallest value that hits the target recall (e.g.
  ≥0.95), document in `helixir/doc/architecture.md` §7. Update HelixDB
  config accordingly.
- **Impact.** Latency vs recall trade-off, exposed and chosen, instead
  of inherited from defaults.

#### P1.5 Real cosine on the **first** ANN response (skip rank-decay proxy)

- **Finding.** `phases.rs:107-130` synthesizes a `vector_score` from
  rank position because "HelixDB intentionally excludes HVector.distance
  from JSON serialization." The current re-rank step then re-embeds the
  candidate content and recomputes cosine.
- **Proposal.** Either:
  - **(a)** Ask the HelixDB team to surface the HNSW distance per result
    (single field, no leakage); then drop the rank-decay proxy and the
    re-embed entirely. This is the highest-leverage change in the entire
    document.
  - **(b)** If (a) is not on the HelixDB roadmap soon, keep the re-rank
    but skip the rank-decay path and use a uniform initial score, since
    the rank-proxy adds noise that the re-rank then has to undo
    (note the `(real_score - hit.vector_score).abs() > 0.01` gate
    silently keeps proxy scores when they happen to agree).
- **Impact.** Removes a whole batch re-embed call per `search_memory`
  in the (a) path — that's the dominant latency on small corpora.

### P2 — polish / future-proofing

#### P2.1 Apply MMR diversification on the final top-K

- **Proposal.** After the fused top-K is selected, run MMR with
  `λ=0.5` on the candidate embeddings. Stops the top-3 from being three
  near-duplicates of the same fact.
- **Impact.** Subjective answer quality from agent perspective.

#### P2.2 Batch enrichment in collective scope

- **Finding.** `dispatch.rs:184-204` runs two HTTP calls per result
  (user count + controversy) under `join_all`. For `k=20` collective
  that's 40 calls.
- **Proposal.** Author `getMemoryEnrichmentBatch(ids, requester)` that
  returns both lists in one round-trip.

#### P2.3 Replace `Vec::pop()` with `VecDeque::pop_front()` in chain BFS

- Smallest possible change to match the comment's claim. Independent of
  P1.2 native BFS plan.

#### P2.4 Cross-encoder reranker on top-K

- **Proposal.** Plug a small reranker (`bge-reranker-v2-m3` is 568 MB,
  good Russian + English) behind a feature flag. Run on top-20 after
  RRF fusion. This is the single biggest precision lever per 2026 RAG
  surveys, but it adds GPU/CPU pressure — feature-flag and benchmark.

#### P2.5 Beam-width Phase-2 expansion

- **Finding.** `phases.rs:366-383` does `take(3)` per level. This is
  greedy and local — a globally promising third-best path might be cut.
- **Proposal.** Maintain a single global priority queue of size B
  (e.g. 8) across all in-flight expansion tasks; pop the most promising
  global frontier next instead of fixed `take(3)` per parent.
- **Impact.** Recall on long reasoning paths.

### P3 — housekeeping

- **P3.1.** Audit the dead retrieval modules from §2.4. For each,
  decide: revive (P1.1 effectively revives `hybrid.rs` + `bm25.rs`),
  delete, or wrap in `<unused reason="...">` like we did with
  `integrator/` and `onto_search/`.
- **P3.2.** Fix the misleading comment that calls the chain walker a
  BFS (`reasoning/chain.rs:1-5`).
- **P3.3.** `SearchEngine::get_stats` / `cache_stats` currently return
  defaults (`traversal.rs:173-174`, `engine.rs:160-161`) — wire the real
  numbers so observability is real.

---

## 7. What this document does not propose

- We **do not** propose moving to IVF-PQ. Helixir's corpus is <10M
  memories; HNSW is the right tool. IVF-PQ is for >100M-scale shops.
- We **do not** propose late-interaction (ColBERT). Helixir stores
  atomic facts (avg <300 chars). ColBERT's win is on long docs.
- We **do not** propose a graph database swap. HelixDB has the right
  primitives; we're using <30% of them today.
- We **do not** open new GitHub issues from this file. When we agree on
  a subset of proposals, open one issue per item with this section as
  the citation source. Severity, labels, and acceptance criteria per
  `AGENTS.md` §4.

---

## 8. References

- HelixDB official docs: <https://docs.helix-db.com/welcome>
- HelixDB `SearchV` / `SearchBM25` / `ShortestPathBFS` / `helix.toml`
  via context7 mirror of `/helixdb/helix-db` (queried 2026-05-12).
- "Hybrid Search for RAG: BM25, SPLADE, and Vector Search Combined",
  Premai blog 2026 — RRF k=60 baseline, normalization caveats.
- "Hybrid Search Explained", Weaviate blog — RRF formula and tradeoffs.
- "IVF vs HNSW Indexing in Milvus", FAUN.dev March 2026 — scale
  thresholds (10M / 100M / 1B).
- "Vector Search & Embeddings Glossary 2026", Digital Applied — current
  vocabulary, reranker hierarchy.
- "Everyone gets bidirectional BFS wrong", zdimension.fr — level-by-level
  advance + frontier-size-aware step pick.
- "Hybrid search with HNSW and BM25 reranking", r/Rag 2026 — RRF in
  production.
- AQR-HNSW (DAC 2026, arXiv:2602.21600) — density-aware quantization +
  multi-stage rerank (relevant only if/when we hit IVF-PQ scale).
- HelixDB benchmarks v1: HelixDB OneHop traversal is 5.9× Neo4j /
  13× Postgres on the public corpus.

---

## 9. Maintenance

- Update §2 whenever a live retrieval pipeline changes shape (new tool,
  retired phase, new HelixDB query name).
- Append §6 proposals as they are implemented or rejected. Strike
  rather than delete — the historical reasoning is the point.
- §3 only changes when HelixDB ships new primitives. Cross-check by
  running `query-docs` against `/helixdb/helix-db` ~quarterly.
