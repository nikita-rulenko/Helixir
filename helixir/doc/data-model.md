# Data model (datadesign)

> _Reflects code as of `v0.6.0-dev`. Last verified: 2026-07-02._

Authoritative source: `helixir/schema/schema.hx` (node + edge definitions)
and `helixir/schema/queries.hx` (153 HQL queries that materialize the
contract). Anything below disagreeing with those files is the bug.

## 1. Storage at a glance

```
                  ┌─────────────────────────────┐
                  │  HelixDB (graph + vector)   │
                  │                             │
                  │   18 node types             │
                  │    + 5 vector-index types   │
                  │   37 edge types             │
                  │     ├── active in code      │
                  │     └── reserved (see §3)   │
                  │   153 named HQL queries     │
                  │   vector dim: 768 (default) │
                  └─────────────────────────────┘
```

There is no relational database, no Redis, no filesystem state. Everything the
service persists lives in HelixDB.

## 2. Node taxonomy

Nodes group into five purposes:

```
                ┌─────────────────┐
                │  Identity / who │   User, Agent, Session
                └─────────────────┘
                ┌─────────────────┐
                │  Content / what │   Memory, MemoryChunk
                └─────────────────┘
                ┌─────────────────┐
                │  Semantics      │   Entity, Concept, Context
                └─────────────────┘
                ┌─────────────────┐
                │  Reasoning      │   Reasoning, Constraint, HistoryEvent
                └─────────────────┘
                ┌─────────────────┐
                │  Vectors        │   MemoryEmbedding, EntityEmbedding,
                │                 │   ChunkEmbedding, ConceptEmbedding
                └─────────────────┘
                ┌─────────────────┐
                │  Doc pipeline   │   DocPage, DocChunk, CodeExample,
                │  (reserved)     │   ErrorCode
                └─────────────────┘
```

| Node | Key fields | Notes |
|---|---|---|
| **User** | `user_id`, `name`, `email`, `created_at`, `metadata` | One per identity. |
| **Agent** | `agent_id`, `role`, `capabilities`, `agent_version`, `host`, `last_seen`, `status` | Tracks which agent wrote which memory — and, since #39, doubles as the swarm presence record: `add_memory(agent_id=…)` heartbeats it (`heartbeatAgent`), `swarm_status` reads the roster. |
| **Session** | `session_id`, `started_at`, `ended_at`, `status`, `session_type` | Reserved — no code path creates Sessions yet. |
| **Memory** | `memory_id`, `user_id`, `content`, `memory_type`, `certainty`, `importance`, `created_at/updated_at`, `valid_from/until`, `immutable`, `verified`, `context_tags`, `source`, `metadata`, `is_deleted/deleted_at/deleted_by`, `user_count` | Core unit. `user_count` is the Hive Memory dedup counter. |
| **MemoryChunk** | `chunk_id`, `position`, `parent_memory_id`, `content`, `token_count` | For long memories split for retrieval. |
| **Entity** | `entity_id`, `name`, `entity_type`, `properties`, `aliases` | LLM-extracted, deduplicated by name/aliases. |
| **Concept** | `concept_id`, `name`, `level`, `description`, `parent_id`, `properties` | Ontology node. `parent_id` denormalizes the `IS_A` edge — see §6. |
| **Context** | `context_id`, `name`, `context_type`, `properties`, `parent_context` | "work", "personal", custom scopes. |
| **Constraint** | `constraint_id`, `rule`, `constraint_type`, `priority`, `active` | Reserved (planned for VALID_IN gating). |
| **Reasoning** | `reasoning_id`, `reasoning_type`, `description`, `confidence` | Reified reasoning step. |
| **HistoryEvent** | `event_id`, `memory_id`, `action`, `old_value`, `new_value`, `timestamp`, `actor` | Audit trail. |
| **MemoryEmbedding** | `content` (proj.), `created_at` | Vector index for memories. |
| **EntityEmbedding** | `name` | Vector index for entities. |
| **ChunkEmbedding** | `embedding: [F64]` | Vector for `DocChunk` (reserved doc pipeline). |
| **ConceptEmbedding** | `embedding: [F64]` | Vector for concept search (reserved). |
| **DocPage / DocChunk / CodeExample / ErrorCode** | — | Reserved doc-ingest pipeline. Schema present, no Rust producer. |

### 2.1 Category subgraph (Clotho, 2026-06)

The controlled-vocabulary substrate the Moirai route over (`d8edc85`). A
deliberate **third axis** over the flat memory graph: a memory's category
membership lets it bridge to distant memories that share it.

| Artifact | Shape | Notes |
|---|---|---|
| **Category** node | `category_id`, `name` (normalized, English-canonical), `kind`, `description`, `created_at` | Dictionary entry. Seeded by `Clotho::seed_dictionary`. |
| **CategoryEmbedding** node | `name` | Vector for embedding-match tagging. *Reserved* — Clotho v0 matches by in-memory cosine (SearchV exposes no readable score), so no producer wires this yet. |
| `TAGGED_AS` edge | Memory → Category, `{confidence, source}` | The tag. `Clotho::auto_tag` (`source="clotho-embed"`). |
| `SUBCATEGORY_OF` edge | Category → Category | Hierarchy (agriculture ⊂ raw-material). Not yet read at query time — Clotho propagates ancestors from the in-memory seed table. |
| `ALIAS_OF` edge | Category → Category | Synonyms (collapses "raw material"/"сырьё"). |
| `CATEGORY_HAS_EMBEDDING` edge | Category → CategoryEmbedding | *Reserved* (see CategoryEmbedding). |

Routing reads: `getMemoryCategories`, `getMemoriesByCategory` (membership +
cross-domain bridge in `connect_memories`); `category_member_ids` feeds
Lachesis PMI subset-overlap (`ln(\|A∩B\|·N / (\|A\|·\|B\|))`). The planned
`Category —CO_OCCURS{count, pmi}→ Category` edge + `Insight` journal nodes are
the next schema step (persists what PMI v0 computes on the fly).

## 3. Edge taxonomy

```
   IDENTITY                CONTENT                     SEMANTICS
   ────────                ────────                    ─────────
   User HAS_MEMORY ───►Memory◄─── HAS_CHUNK ── MemoryChunk
                          │                       │
                          │ MENTIONS ─────────► Entity
                          │ EXTRACTED_ENTITY ─► Entity
                          │ INSTANCE_OF ──────► Concept
                          │ BELONGS_TO_CATEGORY► Concept
                          │ VALID_IN ─────────► Context
                          │ TAGGED_AS ────────► Category
                          │                       │
                          │ HAS_EMBEDDING ────► MemoryEmbedding
   Agent AGENT_CREATED ──►│                       │
                          │ HAS_HISTORY ──────► HistoryEvent
                          │                       │
   REASONING (Memory→Memory):  MEMORY_RELATION{relation_type ∈ 7-type arsenal}
   DECISION  (Memory→Memory):  SUPERSEDES · CONTRADICTS
```

### Memory→memory relations: one physical edge, seven types

The reasoning engine persists ALL typed memory↔memory relations as a single
`MEMORY_RELATION` edge whose `relation_type` property is the type name
(`ReasoningType::edge_name()`), so extending the arsenal needs no schema
change. Four types are causal/logical — `IMPLIES`, `BECAUSE`, `CONTRADICTS`,
`SUPPORTS` — and are what `search_reasoning_chain` walks; three are
associative/structural — `RELATES_TO`, `PART_OF`, `IS_A` — surfaced by
`get_memory_graph` without a causal claim.
(`src/toolkit/mind_toolbox/reasoning/types.rs`, `edges.rs`;
query `addMemoryRelation`.)

Separately, the **decision engine** writes two dedicated edges:

| Edge | Properties | Created in |
|---|---|---|
| `SUPERSEDES` | `reason`, `superseded_at`, `is_contradiction` | decision verdict `SUPERSEDE` (`addMemorySupersession`) |
| `CONTRADICTS` | `resolution`, `resolved`, `resolution_strategy` | verdict `CONTRADICT` / cross-user contradiction (`addMemoryContradiction`); `resolved`/`resolution_strategy` are what the Atropos reconcile pass flips |

(The schema still declares dedicated `IMPLIES`/`BECAUSE`/`SUPPORTS` edges
with HQL ready, but no Rust producer uses them — the arsenal rides
`MEMORY_RELATION`.)

### Active edges

| Edge | From → To | Properties | Created in |
|---|---|---|---|
| `HAS_MEMORY` | User → Memory | `context`, `access_count` | `add_pipeline.rs::link_user_to_memory_bg`; consensus `user_count` derives from these (#54) |
| `INSTANCE_OF` | Memory → Concept | `confidence` | ontology mapping in add pipeline |
| `BELONGS_TO_CATEGORY` | Memory → Concept | `relevance` | ontology mapping |
| `MENTIONS` | Memory → Entity | `salience`, `sentiment` | entity manager |
| `EXTRACTED_ENTITY` | Memory → Entity | `confidence`, `method` | extractor output |
| `RELATES_TO` | Entity → Entity | `relationship_type`, `strength`, `bidirectional` | extractor relations |
| `VALID_IN` | Memory → Context | `priority`, `exclusive` | `add_pipeline/context_link.rs` (creates the Context on miss) |
| `AGENT_CREATED` | Agent → Memory | `timestamp`, `method` | tooling helpers — ensure-then-link: the Agent node is auto-created on first sight |
| `HAS_HISTORY` | Memory → HistoryEvent | — | every UPDATE/SUPERSEDE/DELETE |
| `HAS_CHUNK` | Memory → MemoryChunk | `chunk_index` | chunking manager |
| `NEXT_CHUNK` | MemoryChunk → MemoryChunk | — | chunking manager |
| `CHUNK_HAS_EMBEDDING` | MemoryChunk → MemoryEmbedding | `embedding_model` | chunking manager |
| `MEMORY_RELATION` | Memory → Memory | `relation_type`, `strength`, `created_at`, `metadata` | reasoning engine — see above |
| `SUPERSEDES` / `CONTRADICTS` | Memory → Memory | see above | decision engine — see above |
| `HAS_EMBEDDING` | Memory → MemoryEmbedding | `embedding_model` | add pipeline |
| `ENTITY_HAS_EMBEDDING` | Entity → EntityEmbedding | `embedding_model` | entity manager |
| `HAS_SUBTYPE` | Concept → Concept | — | ontology loader (seed; self-healing against duplicate trees, #67) |
| `TAGGED_AS` | Memory → Category | `confidence`, `source` | `Clotho::auto_tag` (§2.1) |

### Reserved edges

Schema-declared and HQL-ready, but no Rust producer yet:

`OCCURRED_IN` (Memory→Context event-time linking), `IN_SESSION`,
`CREATED_IN`, `IS_A` (Concept-level), `CONCEPT_RELATED_TO`, `PART_OF`
(Entity-level), `APPLIES_IN`, `CHUNK_MENTIONS_CONCEPT`,
`CONCEPT_HAS_EXAMPLE`, `ERROR_REFERENCES_CONCEPT` — plus the dedicated
`IMPLIES`/`BECAUSE`/`SUPPORTS` declarations noted above and the
`CATEGORY_HAS_EMBEDDING`/`SUBCATEGORY_OF`/`ALIAS_OF` states described
in §2.1.

## 4. Ontology hierarchy (instances of `Concept`)

```
Thing
 ├── Attribute
 │     ├── Fact
 │     ├── Preference
 │     ├── Skill
 │     ├── Goal
 │     ├── Opinion
 │     └── Trait
 ├── Event
 │     ├── Action
 │     ├── Experience
 │     └── Achievement
 ├── Entity
 │     ├── Person
 │     ├── Organization
 │     ├── Location
 │     ├── Object
 │     └── Technology
 ├── Relation
 └── State
```

Loaded once at process boot (`ToolingManager::initialize` →
`OntologyManager::load`). Held in an in-process map; HelixDB is the persistent
copy, but reads at runtime hit the in-memory cache.

The 8 user-facing concept types referenced by `search_by_concept` map to the
leaves under `Attribute` and `Event`: `skill, preference, goal, fact, opinion,
experience, achievement, action`.

## 5. Invariants

These are the assumptions the rest of the code relies on. Violating any of
them is a data-integrity bug.

1. **Memory.user_id is non-empty** for every Memory reachable from `HAS_MEMORY`.
   Schema declares `DEFAULT ""` — see §6 issue #12.
2. **HAS_EMBEDDING is 1:1.** Every Memory has at most one MemoryEmbedding.
   Enforced only by convention; no DB constraint.
3. **SUPERSEDES is acyclic.** The decision engine relies on chasing
   `SUPERSEDES` edges backward to find the live memory.
4. **HAS_CHUNK / NEXT_CHUNK forms a path.** For a chunked Memory, chunks form
   a linear sequence indexed by `chunk_index`.
5. **INSTANCE_OF points to an `Attribute`-subtree or `Event`-subtree leaf.**
   The mapper rejects non-leaf classifications.
6. **CONTRADICTS is symmetric in intent.** Code writes a single directed edge;
   queries that walk contradictions handle both directions.
7. **Hive Memory:** `Memory.user_count` is monotone non-decreasing for any
   given `memory_id`.

## 6. Schema patterns to recognize

These are recurring shapes in `schema.hx` that affect how Rust code reads and
writes the data. Tracked variants of these patterns may have open issues —
listed here so contributors recognize them without re-deriving from grep.

- **Booleans encoded as `I64`.** `immutable`, `verified`, `is_deleted`,
  `active`, `resolved`, `bidirectional`, `exclusive`. HelixDB has no `Bool`
  type; convention is `0 = false, 1 = true`.
- **Identity fields with `DEFAULT ""`.** `Memory.user_id`, `Memory.deleted_at`,
  `Memory.deleted_by`. An insert without `user_id` is legal at the schema
  level and produces a node with empty `user_id`.
- **JSON-in-string.** `Memory.metadata`, `Entity.properties`, `Entity.aliases`,
  `Concept.properties` are `String` columns holding serialized JSON. No
  schema validation; every read pays a JSON parse.
- **Time-type variation.** `Memory.created_at` is `String DEFAULT
  "{{timestamp}}"`, while `MemoryEmbedding.created_at` is `Date`.
- **Denormalized parent links.** `Concept.parent_id: String` exists alongside
  the `IS_A` edge.
- **`smart_traversal` module name.** The `_v2` suffix is a naming artifact
  from an earlier `smart_traversal` that was removed; the current module is
  the only implementation.

## 7. Migration approach (for future schema changes)

There is no automated migration framework today. The current playbook is:

1. Edit `schema.hx` and `queries.hx`.
2. Run `helixir-deploy --host … --port … --schema-dir helixir/schema`.
3. HelixDB accepts the new schema but does not migrate existing data;
   adding a non-nullable field to a populated node is therefore not safe.

If a migration framework is needed in the future, the likely shape is a
per-tag set of HQL scripts plus an upgrade tool. No such tool exists today.
