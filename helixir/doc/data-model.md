# Data model (datadesign)

> _Reflects code as of `v0.3.1-fix`. Last verified: 2026-05-12._

Authoritative source: `helixir/schema/schema.hx` (node + edge definitions)
and `helixir/schema/queries.hx` (~100 HQL queries that materialize the
contract). Anything below disagreeing with those files is the bug.

## 1. Storage at a glance

```
                  ┌─────────────────────────────┐
                  │  HelixDB (graph + vector)   │
                  │                             │
                  │   15 node types             │
                  │   33 edge types             │
                  │     ├── 24 active in code   │
                  │     └──  9 reserved         │
                  │   ~100 named HQL queries    │
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
| **Agent** | `agent_id`, `role`, `capabilities`, `agent_version` | Tracks which agent wrote which memory. |
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
                          │ OCCURRED_IN ──────► Context
                          │                       │
                          │ HAS_EMBEDDING ────► MemoryEmbedding
   Agent AGENT_CREATED ──►│                       │
                          │ HAS_HISTORY ──────► HistoryEvent
                          │                       │
   REASONING (Memory→Memory):  IMPLIES · BECAUSE · CONTRADICTS · SUPPORTS
                               SUPERSEDES · MEMORY_RELATION
```

### Active edges (24)

| Edge | From → To | Properties | Created in |
|---|---|---|---|
| `HAS_MEMORY` | User → Memory | `context`, `access_count` | `add_pipeline.rs::link_user_to_memory_bg` |
| `INSTANCE_OF` | Memory → Concept | `confidence` | ontology mapping in add pipeline |
| `BELONGS_TO_CATEGORY` | Memory → Concept | `relevance` | ontology mapping |
| `MENTIONS` | Memory → Entity | `salience`, `sentiment` | entity manager |
| `EXTRACTED_ENTITY` | Memory → Entity | `confidence`, `method` | extractor output |
| `RELATES_TO` | Entity → Entity | `relationship_type`, `strength`, `bidirectional` | extractor relations |
| `VALID_IN` | Memory → Context | `priority`, `exclusive` | context linking |
| `OCCURRED_IN` | Memory → Context | `timestamp` | context linking |
| `AGENT_CREATED` | Agent → Memory | `timestamp`, `method` | tooling helpers |
| `HAS_HISTORY` | Memory → HistoryEvent | — | every UPDATE/SUPERSEDE/DELETE |
| `HAS_CHUNK` | Memory → MemoryChunk | `chunk_index` | chunking manager |
| `NEXT_CHUNK` | MemoryChunk → MemoryChunk | — | chunking manager |
| `CHUNK_HAS_EMBEDDING` | MemoryChunk → MemoryEmbedding | `embedding_model` | chunking manager |
| `MEMORY_RELATION` | Memory → Memory | `relation_type`, `strength`, `created_at`, `metadata` | reasoning engine (generic) |
| `IMPLIES` | Memory → Memory | `probability`, `reasoning_id` | reasoning engine |
| `BECAUSE` | Memory → Memory | `strength`, `reasoning_id` | reasoning engine |
| `CONTRADICTS` | Memory → Memory | `resolution`, `resolved`, `resolution_strategy` | decision engine `CONTRADICT` / `CROSS_CONTRADICT` |
| `SUPERSEDES` | Memory → Memory | `reason`, `superseded_at`, `is_contradiction` | decision engine `SUPERSEDE` |
| `HAS_EMBEDDING` | Memory → MemoryEmbedding | `embedding_model` | add pipeline |
| `ENTITY_HAS_EMBEDDING` | Entity → EntityEmbedding | `embedding_model` | entity manager |
| `HAS_SUBTYPE` | Concept → Concept | — | ontology loader (seed) |
| `PAGE_TO_CHUNK` | DocPage → DocChunk | — | reserved |
| `CHUNK_TO_EMBEDDING` | DocChunk → ChunkEmbedding | — | reserved |
| `SUPPORTS` | Memory → Memory | — | reasoning engine |

### Reserved edges (9)

Schema-declared and HQL-ready, but no Rust producer yet:

`IN_SESSION`, `CREATED_IN`, `IS_A`, `CONCEPT_RELATED_TO`, `PART_OF`,
`APPLIES_IN`, `CHUNK_MENTIONS_CONCEPT`, `CONCEPT_HAS_EXAMPLE`,
`ERROR_REFERENCES_CONCEPT`.

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

## 6. Known data-model debt

These are the recurring schema smells. They are tracked as live issues; this
file lists the patterns so contributors can recognize them without grep.

- **Booleans encoded as `I64`.** `immutable`, `verified`, `is_deleted`,
  `active`, `resolved`, `bidirectional`, `exclusive`. HelixDB has no `Bool`
  type; convention is `0 = false, 1 = true`.
- **Identity fields with `DEFAULT ""`.** `Memory.user_id`, `Memory.deleted_at`,
  `Memory.deleted_by`. An insert without `user_id` is legal at the schema
  level and silently produces an orphan.
- **JSON-in-string.** `Memory.metadata`, `Entity.properties`, `Entity.aliases`,
  `Concept.properties` are `String` columns holding serialized JSON. No
  schema validation; every read pays a JSON parse.
- **Time-type drift.** `Memory.created_at` is `String DEFAULT "{{timestamp}}"`,
  but `MemoryEmbedding.created_at` is `Date`. Cross-node date queries are
  fragile.
- **Denormalized parent links.** `Concept.parent_id: String` duplicates the
  `IS_A` edge; two sources of truth for parenthood.
- **`smart_traversal_v2` artifact.** Naming implies a v1 elsewhere; v1 was
  removed without renaming.

## 7. Migration approach (for future schema changes)

There is no migration framework today. The current playbook is:

1. Edit `schema.hx` and `queries.hx`.
2. Run `helixir-deploy --host … --port … --schema-dir helixir/schema`.
3. HelixDB will accept the new schema but **does not migrate existing data**;
   adding a non-nullable field to a populated node is therefore not safe.

This is acceptable while the project has no production deployment. Before that
changes, a real migration layer needs to be added — likely a per-tag set of
HQL scripts plus an upgrade tool.
