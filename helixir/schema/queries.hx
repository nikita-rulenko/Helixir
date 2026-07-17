QUERY addUser(user_id: String, name: String) =>
  user <- AddN<User>({ user_id: user_id, name: name })
  RETURN user
QUERY getUser(user_id: String) =>
  user <- N<User>::WHERE(_::{user_id}::EQ(user_id))::FIRST
  RETURN user
QUERY addMemory(memory_id: String, user_id: String, content: String, memory_type: String, certainty: I64, importance: I64, created_at: String, updated_at: String, context_tags: String, source: String, metadata: String) =>
  memory <- AddN<Memory>({ memory_id: memory_id, user_id: user_id, content: content, memory_type: memory_type, certainty: certainty, importance: importance, created_at: created_at, updated_at: updated_at, context_tags: context_tags, source: source, metadata: metadata })
  RETURN memory
// addMemoryWithValidFrom: like addMemory but also sets valid_from explicitly.
// The schema default `valid_from: String DEFAULT "{{timestamp}}"` is a literal,
// not a macro (HelixDB's only timestamp default is `DEFAULT NOW`, valid on Date
// fields only — see #45), so an unset String valid_from persists "{{timestamp}}".
// Passing it here keeps valid_from a real RFC3339 timestamp without a Date-type
// migration. Additive — addMemory stays for backward compatibility.
QUERY addMemoryWithValidFrom(memory_id: String, user_id: String, content: String, memory_type: String, certainty: I64, importance: I64, created_at: String, updated_at: String, valid_from: String, context_tags: String, source: String, metadata: String) =>
  memory <- AddN<Memory>({ memory_id: memory_id, user_id: user_id, content: content, memory_type: memory_type, certainty: certainty, importance: importance, created_at: created_at, updated_at: updated_at, valid_from: valid_from, context_tags: context_tags, source: source, metadata: metadata })
  RETURN memory
// #43: atomic content-keyed dedup. UpsertN collapses concurrent identical
// writes onto ONE canonical Memory (keyed by the INDEX'd content_key), so the
// read-after-write snapshot lag can't fork it into duplicates; UpsertE makes the
// per-user HAS_MEMORY link idempotent so the derived user_count stays correct.
// memory_id must be deterministic (= a function of content_key) so the upsert's
// update branch is a no-op on identity.
QUERY addOrLinkMemoryByContentKey(content_key: String, memory_id: String, user_id: String, content: String, memory_type: String, certainty: I64, importance: I64, created_at: String, updated_at: String, context_tags: String, source: String, metadata: String, stance: String, linked_at: String) =>
  user <- N<User>::WHERE(_::{user_id}::EQ(user_id))::FIRST
  existing_mem <- N<Memory>::WHERE(_::{content_key}::EQ(content_key))
  memory <- existing_mem::UpsertN({ memory_id: memory_id, content_key: content_key, user_id: user_id, content: content, memory_type: memory_type, certainty: certainty, importance: importance, created_at: created_at, updated_at: updated_at, context_tags: context_tags, source: source, metadata: metadata })
  existing_link <- E<HAS_MEMORY>
  link <- existing_link::UpsertE({ context: context_tags, access_count: 0, stance: stance, certainty: certainty, linked_at: linked_at, last_confirmed: linked_at })::From(user)::To(memory)
  RETURN memory
// #54: derive user_count from the live HAS_MEMORY edge set instead of the
// read-then-write scalar (a lost-update under concurrent linkers).
QUERY getMemoryUserCount(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  count <- memory::In<HAS_MEMORY>::COUNT
  RETURN count
// #43 (personal-node + subscribe-by-fingerprint model): each user keeps their own
// Memory node; identical facts share a content_key. Collective consensus is the
// count of users holding any node in the content_key group — derived on read, so
// there is no shared node to race on. Additive: addMemoryWithValidFrom stays.
QUERY addMemoryKeyed(memory_id: String, content_key: String, user_id: String, content: String, memory_type: String, certainty: I64, importance: I64, created_at: String, updated_at: String, valid_from: String, context_tags: String, source: String, metadata: String) =>
  memory <- AddN<Memory>({ memory_id: memory_id, content_key: content_key, user_id: user_id, content: content, memory_type: memory_type, certainty: certainty, importance: importance, created_at: created_at, updated_at: updated_at, valid_from: valid_from, context_tags: context_tags, source: source, metadata: metadata })
  RETURN memory
// Consensus over a fingerprint group: how many distinct holders across all
// personal nodes that share this content_key.
QUERY getContentKeyGroupUserCount(content_key: String) =>
  holders <- N<Memory>::WHERE(_::{content_key}::EQ(content_key))::In<HAS_MEMORY>
  count <- holders::COUNT
  RETURN count
// All personal nodes for a fingerprint group (collective view groups by these).
QUERY getMemoriesByContentKey(content_key: String) =>
  memories <- N<Memory>::WHERE(_::{content_key}::EQ(content_key))
  RETURN memories
// Backfill: stamp a content_key fingerprint onto an existing node (hash is
// computed in Rust; HelixDB only stores it).
QUERY setMemoryContentKey(memory_id: String, content_key: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  updated <- memory::UPDATE({ content_key: content_key })
  RETURN updated
QUERY getMemory(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  RETURN memory
QUERY getRecentMemories(limit: I64) =>
  memories <- N<Memory>::RANGE(0, limit)
  RETURN memories
QUERY linkUserToMemory(user_id: String, memory_id: String, context: String) =>
  user <- N<User>::WHERE(_::{user_id}::EQ(user_id))::FIRST
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  link <- AddE<HAS_MEMORY>({ context: context, access_count: 0 })::From(user)::To(memory)
  RETURN link
QUERY addContext(context_id: String, name: String, context_type: String, properties: String, parent_context: String) =>
  context <- AddN<Context>({ context_id: context_id, name: name, context_type: context_type, properties: properties, parent_context: parent_context })
  RETURN context
QUERY getContext(context_id: String) =>
  context <- N<Context>::WHERE(_::{context_id}::EQ(context_id))::FIRST
  RETURN context
QUERY getRecentContexts(limit: I64) =>
  contexts <- N<Context>::RANGE(0, limit)
  RETURN contexts
QUERY updateMemory(memory_id: String, content: String, certainty: I64, importance: I64, updated_at: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  updated <- memory::UPDATE({ content: content, certainty: certainty, importance: importance, updated_at: updated_at })
  RETURN updated
QUERY updateMemoryById(id: ID, content: String, certainty: I64, importance: I64, updated_at: String) =>
  updated <- N<Memory>(id)::UPDATE({ content: content, certainty: certainty, importance: importance, updated_at: updated_at })
  RETURN updated
QUERY deleteMemoryEmbedding(memory_id: ID) =>
  DROP N<Memory>(memory_id)::Out<HAS_EMBEDDING>
  RETURN "deleted"
QUERY getMemoryEmbedding(memory_id: ID) =>
  embedding <- N<Memory>(memory_id)::Out<HAS_EMBEDDING>::FIRST
  RETURN embedding
QUERY addMemoryRelation(source_id: String, target_id: String, relation_type: String, strength: I64, created_at: String, metadata: String) =>
  source <- N<Memory>::WHERE(_::{memory_id}::EQ(source_id))::FIRST
  target <- N<Memory>::WHERE(_::{memory_id}::EQ(target_id))::FIRST
  relation <- AddE<MEMORY_RELATION>({ relation_type: relation_type, strength: strength, created_at: created_at, metadata: metadata })::From(source)::To(target)
  RETURN relation
QUERY getRelatedMemories(memory_id: ID) =>
  memory <- N<Memory>(memory_id)
  related <- memory::Out<MEMORY_RELATION>
  RETURN related
QUERY addMemoryImplication(from_id: String, to_id: String, probability: I64, reasoning_id: String) =>
  from_memory <- N<Memory>::WHERE(_::{memory_id}::EQ(from_id))::FIRST
  to_memory <- N<Memory>::WHERE(_::{memory_id}::EQ(to_id))::FIRST
  implication <- AddE<IMPLIES>({ probability: probability, reasoning_id: reasoning_id })::From(from_memory)::To(to_memory)
  RETURN implication
QUERY addMemoryCausation(from_id: String, to_id: String, strength: I64, reasoning_id: String) =>
  from_memory <- N<Memory>::WHERE(_::{memory_id}::EQ(from_id))::FIRST
  to_memory <- N<Memory>::WHERE(_::{memory_id}::EQ(to_id))::FIRST
  causation <- AddE<BECAUSE>({ strength: strength, reasoning_id: reasoning_id })::From(from_memory)::To(to_memory)
  RETURN causation
QUERY addMemoryContradiction(from_id: String, to_id: String, resolution: String, resolved: I64, resolution_strategy: String) =>
  from_memory <- N<Memory>::WHERE(_::{memory_id}::EQ(from_id))::FIRST
  to_memory <- N<Memory>::WHERE(_::{memory_id}::EQ(to_id))::FIRST
  contradiction <- AddE<CONTRADICTS>({ resolution: resolution, resolved: resolved, resolution_strategy: resolution_strategy })::From(from_memory)::To(to_memory)
  RETURN contradiction

// Contradiction-debt reconciliation (#45): enumerate a memory's outgoing
// disputes (edges + their targets, parallel order) so the Cutter can drain the
// dead ones; and retire the open ones from a memory with a strategy label.
QUERY getMemoryContradictionsFull(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  out_edges <- memory::OutE<CONTRADICTS>
  out_targets <- memory::Out<CONTRADICTS>
  RETURN out_edges, out_targets

QUERY resolveMemoryContradictions(memory_id: String, strategy: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  edges <- memory::OutE<CONTRADICTS>::WHERE(_::{resolved}::EQ(0))
  updated <- edges::UPDATE({ resolved: 1, resolution_strategy: strategy })
  RETURN updated
QUERY addMemorySupersession(new_id: String, old_id: String, reason: String, superseded_at: String, is_contradiction: I64) =>
  new_memory <- N<Memory>::WHERE(_::{memory_id}::EQ(new_id))::FIRST
  old_memory <- N<Memory>::WHERE(_::{memory_id}::EQ(old_id))::FIRST
  supersedes <- AddE<SUPERSEDES>({ reason: reason, superseded_at: superseded_at, is_contradiction: is_contradiction })::From(new_memory)::To(old_memory)
  RETURN supersedes
QUERY getSupersededMemories(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  superseded <- memory::Out<SUPERSEDES>
  RETURN superseded
QUERY getSupersededBatch(memory_ids: [String]) =>
  memories <- N<Memory>::WHERE(_::{memory_id}::IS_IN(memory_ids))
  superseded_edges <- memories::InE<SUPERSEDES>
  successors <- memories::In<SUPERSEDES>
  RETURN memories, superseded_edges, successors
QUERY getSupersedingMemory(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  superseding <- memory::In<SUPERSEDES>
  RETURN superseding
QUERY getMemoryOutgoingRelations(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  implies_out <- memory::OutE<IMPLIES>
  because_out <- memory::OutE<BECAUSE>
  relations_out <- memory::OutE<MEMORY_RELATION>
  RETURN implies_out, because_out, relations_out
QUERY getMemoryIncomingRelations(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  implies_in <- memory::InE<IMPLIES>
  because_in <- memory::InE<BECAUSE>
  relations_in <- memory::InE<MEMORY_RELATION>
  RETURN implies_in, because_in, relations_in
QUERY addReasoningRelation(relation_id: String, from_memory_id: String, to_memory_id: String, relation_type: String, strength: I64, confidence: I64, explanation: String, created_by: String, created_at: String) =>
  from_mem <- N<Memory>::WHERE(_::{memory_id}::EQ(from_memory_id))::FIRST
  to_mem <- N<Memory>::WHERE(_::{memory_id}::EQ(to_memory_id))::FIRST
  relation <- AddE<MEMORY_RELATION>({ relation_type: relation_type, strength: strength, created_at: created_at, metadata: "" })::From(from_mem)::To(to_mem)
  RETURN relation
QUERY addMemoryEmbedding(memory_id: ID, vector_data: [F64], embedding_model: String, created_at: Date) =>
  embedding <- AddV<MemoryEmbedding>(vector_data, { created_at: created_at })
  link <- AddE<HAS_EMBEDDING>({ embedding_model: embedding_model })::From(memory_id)::To(embedding)
  RETURN embedding
QUERY getMemoryByEmbeddingId(embedding_id: ID) =>
  embedding <- V<MemoryEmbedding>(embedding_id)
  memory <- embedding::In<HAS_EMBEDDING>
  RETURN memory
QUERY addEntityEmbedding(entity_id: String, vector_data: [F64], content: String, embedding_model: String) =>
  entity <- N<Entity>::WHERE(_::{entity_id}::EQ(entity_id))::FIRST
  embedding <- AddV<EntityEmbedding>(vector_data, { name: content })
  link <- AddE<ENTITY_HAS_EMBEDDING>({ embedding_model: embedding_model })::From(entity)::To(embedding)
  RETURN embedding
QUERY getEntity(entity_id: String) =>
  entity <- N<Entity>::WHERE(_::{entity_id}::EQ(entity_id))::FIRST
  RETURN entity
QUERY getEntityByName(name: String) =>
  entity <- N<Entity>::WHERE(_::{name}::EQ(name))::FIRST
  RETURN entity
QUERY createEntity(entity_id: String, name: String, entity_type: String, properties: String, aliases: String) =>
  entity <- AddN<Entity>({
    entity_id: entity_id,
    name: name,
    entity_type: entity_type,
    properties: properties,
    aliases: aliases
  })
  RETURN entity
QUERY linkExtractedEntity(memory_id: String, entity_id: String, confidence: I64, method: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  entity <- N<Entity>::WHERE(_::{entity_id}::EQ(entity_id))::FIRST
  link <- AddE<EXTRACTED_ENTITY>({ confidence: confidence, method: method })::From(memory)::To(entity)
  RETURN link
QUERY linkMentionsEntity(memory_id: String, entity_id: String, salience: I64, sentiment: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  entity <- N<Entity>::WHERE(_::{entity_id}::EQ(entity_id))::FIRST
  link <- AddE<MENTIONS>({ salience: salience, sentiment: sentiment })::From(memory)::To(entity)
  RETURN link
QUERY linkMemoryToInstanceOf(memory_id: String, concept_id: String, confidence: I64) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  concept <- N<Concept>::WHERE(_::{concept_id}::EQ(concept_id))::FIRST
  link <- AddE<INSTANCE_OF>({ confidence: confidence })::From(memory)::To(concept)
  RETURN link
QUERY searchSimilarMemories(query_vector: [F64], limit: I64) =>
  embeddings <- SearchV<MemoryEmbedding>(query_vector, limit)
  RETURN embeddings
QUERY vectorSearch(query_vector: [F64], user_id: String, limit: I64, min_score: F64) =>
  embeddings <- SearchV<MemoryEmbedding>(query_vector, limit)
  RETURN embeddings
QUERY smartVectorSearchWithChunks(query_vector: [F64], limit: I64) =>
  embeddings <- SearchV<MemoryEmbedding>(query_vector, limit)
  memories <- embeddings::In<HAS_EMBEDDING>
  RETURN memories
QUERY smartVectorSearchWithChunksCutoff(query_vector: [F64], limit: I64, cutoff_date: Date) =>
  embeddings <- SearchV<MemoryEmbedding>(query_vector, limit)::WHERE(_::{created_at}::GTE(cutoff_date))
  memories <- embeddings::In<HAS_EMBEDDING>
  RETURN memories
QUERY searchMemoriesByBm25(text: String, limit: I64) =>
  memories <- SearchBM25<Memory>(text, limit)
  RETURN memories
QUERY searchSimilarEntities(query_vector: [F64], limit: I64) =>
  embeddings <- SearchV<EntityEmbedding>(query_vector, limit)
  RETURN embeddings
QUERY searchRecentMemories(query_vector: [F64], limit: I64, cutoff_date: Date) =>
  embeddings <- SearchV<MemoryEmbedding>(query_vector, limit)::WHERE(_::{created_at}::GTE(cutoff_date))
  RETURN embeddings
QUERY addMemoryChunk(chunk_id: String, parent_memory_id: String, position: I64, content: String, token_count: I64, created_at: String) =>
  chunk <- AddN<MemoryChunk>({ chunk_id: chunk_id, parent_memory_id: parent_memory_id, position: position, content: content, token_count: token_count, created_at: created_at })
  parent <- N<Memory>::WHERE(_::{memory_id}::EQ(parent_memory_id))::FIRST
  link <- AddE<HAS_CHUNK>({ chunk_index: position })::From(parent)::To(chunk)
  RETURN chunk
QUERY addDocPage(url: String, title: String, category: String, word_count: I64) =>
  page <- AddN<DocPage>({ url: url, title: title, category: category, word_count: word_count })
  RETURN page
QUERY addChunkEmbedding(chunk_id: String, vector_data: [F64]) =>
  chunk <- N<DocChunk>::WHERE(_::{chunk_id}::EQ(chunk_id))::FIRST
  embedding <- AddV<ChunkEmbedding>(vector_data)
  link <- AddE<CHUNK_TO_EMBEDDING>::From(chunk)::To(embedding)
  RETURN embedding
QUERY searchDocChunks(query_vector: [F64], limit: I64) =>
  embeddings <- SearchV<ChunkEmbedding>(query_vector, limit)
  RETURN embeddings
QUERY addCodeExample(example_id: String, code: String, language: String, description: String) =>
  example <- AddN<CodeExample>({ example_id: example_id, code: code, language: language, description: description })
  RETURN example
QUERY searchConceptsByName(name: String) =>
  concepts <- N<Concept>::WHERE(_::{name}::EQ(name))
  RETURN concepts

QUERY checkOntologyInitialized() =>
  thing <- N<Concept>::WHERE(_::{concept_id}::EQ("Thing"))::FIRST
  RETURN thing

QUERY getConceptByID(concept_id: String) =>
  concept <- N<Concept>::WHERE(_::{concept_id}::EQ(concept_id))::FIRST
  RETURN concept

QUERY getAllConcepts() =>
  concepts <- N<Concept>
  RETURN concepts

QUERY getConceptSubtypes(concept_id: String) =>
  parent <- N<Concept>::WHERE(_::{concept_id}::EQ(concept_id))::FIRST
  subtypes <- parent::Out<HAS_SUBTYPE>
  RETURN subtypes

QUERY initializeBaseOntology() =>
  thing <- AddN<Concept>({
    concept_id: "Thing",
    name: "Thing",
    level: 1,
    description: "The most general concept",
    parent_id: "",
    properties: "{}"
  })
  
  attribute <- AddN<Concept>({
    concept_id: "Attribute",
    name: "Attribute",
    level: 2,
    description: "A characteristic or property",
    parent_id: "Thing",
    properties: "{}"
  })
  
  event <- AddN<Concept>({
    concept_id: "Event",
    name: "Event",
    level: 2,
    description: "Something that happens",
    parent_id: "Thing",
    properties: "{}"
  })
  
  entity <- AddN<Concept>({
    concept_id: "Entity",
    name: "Entity",
    level: 2,
    description: "A distinct independent existence",
    parent_id: "Thing",
    properties: "{}"
  })
  
  relation <- AddN<Concept>({
    concept_id: "Relation",
    name: "Relation",
    level: 2,
    description: "A connection between entities or concepts",
    parent_id: "Thing",
    properties: "{}"
  })
  
  state <- AddN<Concept>({
    concept_id: "State",
    name: "State",
    level: 2,
    description: "A condition or mode of being",
    parent_id: "Thing",
    properties: "{}"
  })
  
  edge1 <- AddE<HAS_SUBTYPE>::From(thing)::To(attribute)
  edge2 <- AddE<HAS_SUBTYPE>::From(thing)::To(event)
  edge3 <- AddE<HAS_SUBTYPE>::From(thing)::To(entity)
  edge4 <- AddE<HAS_SUBTYPE>::From(thing)::To(relation)
  edge5 <- AddE<HAS_SUBTYPE>::From(thing)::To(state)
  
  preference <- AddN<Concept>({
    concept_id: "Preference",
    name: "Preference",
    level: 3,
    description: "A strong liking or disliking",
    parent_id: "Attribute",
    properties: "{}"
  })
  
  skill <- AddN<Concept>({
    concept_id: "Skill",
    name: "Skill",
    level: 3,
    description: "An ability to do something well",
    parent_id: "Attribute",
    properties: "{}"
  })
  
  fact <- AddN<Concept>({
    concept_id: "Fact",
    name: "Fact",
    level: 3,
    description: "A piece of information presented as true",
    parent_id: "Attribute",
    properties: "{}"
  })
  
  opinion <- AddN<Concept>({
    concept_id: "Opinion",
    name: "Opinion",
    level: 3,
    description: "A view or judgment formed about something",
    parent_id: "Attribute",
    properties: "{}"
  })
  
  goal <- AddN<Concept>({
    concept_id: "Goal",
    name: "Goal",
    level: 3,
    description: "The object of a person's ambition or effort",
    parent_id: "Attribute",
    properties: "{}"
  })
  
  trait_concept <- AddN<Concept>({
    concept_id: "Trait",
    name: "Trait",
    level: 3,
    description: "A distinguishing quality or characteristic",
    parent_id: "Attribute",
    properties: "{}"
  })
  
  edge6 <- AddE<HAS_SUBTYPE>::From(attribute)::To(preference)
  edge7 <- AddE<HAS_SUBTYPE>::From(attribute)::To(skill)
  edge8 <- AddE<HAS_SUBTYPE>::From(attribute)::To(fact)
  edge9 <- AddE<HAS_SUBTYPE>::From(attribute)::To(opinion)
  edge10 <- AddE<HAS_SUBTYPE>::From(attribute)::To(goal)
  edge11 <- AddE<HAS_SUBTYPE>::From(attribute)::To(trait_concept)
  
  action <- AddN<Concept>({
    concept_id: "Action",
    name: "Action",
    level: 3,
    description: "The process of doing something",
    parent_id: "Event",
    properties: "{}"
  })
  
  experience <- AddN<Concept>({
    concept_id: "Experience",
    name: "Experience",
    level: 3,
    description: "Practical contact with and observation of facts or events",
    parent_id: "Event",
    properties: "{}"
  })
  
  achievement <- AddN<Concept>({
    concept_id: "Achievement",
    name: "Achievement",
    level: 3,
    description: "A thing done successfully typically by effort courage or skill",
    parent_id: "Event",
    properties: "{}"
  })
  
  edge12 <- AddE<HAS_SUBTYPE>::From(event)::To(action)
  edge13 <- AddE<HAS_SUBTYPE>::From(event)::To(experience)
  edge14 <- AddE<HAS_SUBTYPE>::From(event)::To(achievement)
  
  person <- AddN<Concept>({
    concept_id: "Person",
    name: "Person",
    level: 3,
    description: "A human being",
    parent_id: "Entity",
    properties: "{}"
  })
  
  organization <- AddN<Concept>({
    concept_id: "Organization",
    name: "Organization",
    level: 3,
    description: "An organized body of people with a particular purpose",
    parent_id: "Entity",
    properties: "{}"
  })
  
  location <- AddN<Concept>({
    concept_id: "Location",
    name: "Location",
    level: 3,
    description: "A place or position",
    parent_id: "Entity",
    properties: "{}"
  })
  
  object_concept <- AddN<Concept>({
    concept_id: "Object",
    name: "Object",
    level: 3,
    description: "A material thing that can be seen and touched",
    parent_id: "Entity",
    properties: "{}"
  })
  
  technology <- AddN<Concept>({
    concept_id: "Technology",
    name: "Technology",
    level: 3,
    description: "Tools, systems, methods, or techniques used to solve problems or achieve goals",
    parent_id: "Entity",
    properties: "{}"
  })
  
  edge15 <- AddE<HAS_SUBTYPE>::From(entity)::To(person)
  edge16 <- AddE<HAS_SUBTYPE>::From(entity)::To(organization)
  edge17 <- AddE<HAS_SUBTYPE>::From(entity)::To(location)
  edge18 <- AddE<HAS_SUBTYPE>::From(entity)::To(object_concept)
  edge19 <- AddE<HAS_SUBTYPE>::From(entity)::To(technology)
  
  RETURN thing

QUERY linkMemoryToChunk(memory_id: String, chunk_id: String, chunk_index: I64) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  chunk <- N<MemoryChunk>::WHERE(_::{chunk_id}::EQ(chunk_id))::FIRST
  link <- AddE<HAS_CHUNK>({ chunk_index: chunk_index })::From(memory)::To(chunk)
  RETURN link

QUERY getMemoryChunks(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  chunks <- memory::Out<HAS_CHUNK>
  RETURN chunks

QUERY getMemoryWithChunks(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  chunks <- memory::Out<HAS_CHUNK>
  RETURN memory, chunks

QUERY getUserMemories(user_id: String, limit: I64) =>
  user <- N<User>::WHERE(_::{user_id}::EQ(user_id))::FIRST
  memories <- user::Out<HAS_MEMORY>::RANGE(0, limit)
  RETURN memories

QUERY getMemoryEntities(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  entities <- memory::Out<EXTRACTED_ENTITY>
  mentions <- memory::Out<MENTIONS>
  RETURN entities, mentions
// #33 relation density: memories linked to a given entity via EXTRACTED_ENTITY,
// excluding `exclude_memory_id`. The cross-domain bridge primitive — the
// background consolidate pass (Clotho-lite) uses it to weave reasoning edges
// between memories that share an entity but are embedding-dissimilar (which
// similarity alone can never surface). Additive — getMemoryEntities is untouched.
QUERY getMemoriesByEntity(entity_id: String, exclude_memory_id: String, limit: I64) =>
  entity <- N<Entity>::WHERE(_::{entity_id}::EQ(entity_id))::FIRST
  memories <- entity::In<EXTRACTED_ENTITY>::WHERE(_::{memory_id}::NEQ(exclude_memory_id))::RANGE(0, limit)
  RETURN memories

QUERY getMemoryConcepts(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  instance_of <- memory::Out<INSTANCE_OF>
  belongs_to <- memory::Out<TAGGED_AS>
  RETURN instance_of, belongs_to

QUERY getMemoryReasoningRelations(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  outgoing <- memory::Out<MEMORY_RELATION>
  incoming <- memory::In<MEMORY_RELATION>
  RETURN outgoing, incoming

QUERY getMemoryLogicalConnections(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  implies_out <- memory::Out<IMPLIES>
  implies_in <- memory::In<IMPLIES>
  because_out <- memory::Out<BECAUSE>
  because_in <- memory::In<BECAUSE>
  contradicts_out <- memory::Out<CONTRADICTS>
  contradicts_in <- memory::In<CONTRADICTS>
  relation_out <- memory::Out<MEMORY_RELATION>
  relation_in <- memory::In<MEMORY_RELATION>
  RETURN implies_out, implies_in, because_out, because_in, contradicts_out, contradicts_in, relation_out, relation_in


QUERY getMemoryGraphStats(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  entities <- memory::Out<EXTRACTED_ENTITY>
  mentions <- memory::Out<MENTIONS>
  concepts <- memory::Out<INSTANCE_OF>
  categories <- memory::Out<TAGGED_AS>
  reasoning_out <- memory::Out<MEMORY_RELATION>
  reasoning_in <- memory::In<MEMORY_RELATION>
  RETURN memory, entities, mentions, concepts, categories, reasoning_out, reasoning_in

QUERY getAllMemories() =>
  memories <- N<Memory>
  RETURN memories

QUERY getAllUsers() =>
  users <- N<User>
  RETURN users

QUERY countAllMemories() =>
  count <- N<Memory>::COUNT
  RETURN count

QUERY countAllUsers() =>
  count <- N<User>::COUNT
  RETURN count

QUERY countAllEntities() =>
  count <- N<Entity>::COUNT
  RETURN count

QUERY countAllConcepts() =>
  count <- N<Concept>::COUNT
  RETURN count

QUERY countUserMemories(user_id: String) =>
  user <- N<User>::WHERE(_::{user_id}::EQ(user_id))::FIRST
  count <- user::Out<HAS_MEMORY>::COUNT
  RETURN count

QUERY searchByContextTag(tag: String, limit: I64) =>
  memories <- N<Memory>::WHERE(_::{context_tags}::EQ(tag))::RANGE(0, limit)
  RETURN memories

QUERY getMemoryUsers(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  users <- memory::In<HAS_MEMORY>
  RETURN users

QUERY updateMemoryUserCount(memory_id: String, user_count: I64, updated_at: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  updated <- memory::UPDATE({ user_count: user_count, updated_at: updated_at })
  RETURN updated

QUERY checkUserMemoryLink(user_id: String, memory_id: String) =>
  user <- N<User>::WHERE(_::{user_id}::EQ(user_id))::FIRST
  memories <- user::Out<HAS_MEMORY>
  RETURN memories

QUERY getMemoryContradictions(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  contradicts_out <- memory::Out<CONTRADICTS>
  contradicts_in <- memory::In<CONTRADICTS>
  RETURN contradicts_out, contradicts_in

QUERY globalVectorSearch(query_vector: [F64], limit: I64) =>
  embeddings <- SearchV<MemoryEmbedding>(query_vector, limit)
  memories <- embeddings::In<HAS_EMBEDDING>
  RETURN memories

QUERY linkMemoryToSession(memory_id: String, session_id: String, sequence: I64) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  session <- N<Session>::WHERE(_::{session_id}::EQ(session_id))::FIRST
  link <- AddE<CREATED_IN>({ sequence: sequence })::From(memory)::To(session)
  RETURN link

QUERY getSessionMemories(session_id: String) =>
  session <- N<Session>::WHERE(_::{session_id}::EQ(session_id))::FIRST
  memories <- session::In<CREATED_IN>
  RETURN memories

QUERY getMemorySession(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  session <- memory::Out<CREATED_IN>
  RETURN session

QUERY addAgent(agent_id: String, name: String, role: String, capabilities: String, agent_version: String, created_at: String) =>
  agent <- AddN<Agent>({ agent_id: agent_id, name: name, role: role, capabilities: capabilities, agent_version: agent_version, created_at: created_at })
  RETURN agent

QUERY getAgent(agent_id: String) =>
  agent <- N<Agent>::WHERE(_::{agent_id}::EQ(agent_id))::FIRST
  RETURN agent

QUERY linkAgentToMemory(agent_id: String, memory_id: String, timestamp: String, method: String) =>
  agent <- N<Agent>::WHERE(_::{agent_id}::EQ(agent_id))::FIRST
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  link <- AddE<AGENT_CREATED>({ timestamp: timestamp, method: method })::From(agent)::To(memory)
  RETURN link

QUERY getAgentMemories(agent_id: String) =>
  agent <- N<Agent>::WHERE(_::{agent_id}::EQ(agent_id))::FIRST
  memories <- agent::Out<AGENT_CREATED>
  RETURN memories

QUERY getMemoryAgent(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  agent <- memory::In<AGENT_CREATED>
  RETURN agent

// Swarm rendezvous (#39): presence lives in the shared graph, so agents on any
// host see each other through the one DB — no CLI-to-CLI coordination.
QUERY dropPresenceByAgentId(agent_id: String) =>
  DROP N<Agent>::WHERE(_::{agent_id}::EQ(agent_id))
  RETURN "deleted"
QUERY heartbeatAgent(agent_id: String, host: String, last_seen: String, status: String) =>
  agent <- N<Agent>::WHERE(_::{agent_id}::EQ(agent_id))::FIRST
  updated <- agent::UPDATE({ host: host, last_seen: last_seen, status: status })
  RETURN updated

QUERY listAgents() =>
  agents <- N<Agent>
  RETURN agents

QUERY addConceptIsA(child_id: String, parent_id: String, inheritance_type: String) =>
  child <- N<Concept>::WHERE(_::{concept_id}::EQ(child_id))::FIRST
  parent <- N<Concept>::WHERE(_::{concept_id}::EQ(parent_id))::FIRST
  link <- AddE<IS_A>({ inheritance_type: inheritance_type })::From(child)::To(parent)
  RETURN link

QUERY getConceptParentsIsA(concept_id: String) =>
  concept <- N<Concept>::WHERE(_::{concept_id}::EQ(concept_id))::FIRST
  parents <- concept::Out<IS_A>
  RETURN parents

QUERY getConceptChildrenIsA(concept_id: String) =>
  concept <- N<Concept>::WHERE(_::{concept_id}::EQ(concept_id))::FIRST
  children <- concept::In<IS_A>
  RETURN children

QUERY addConceptRelation(from_id: String, to_id: String, relation_type: String) =>
  from_concept <- N<Concept>::WHERE(_::{concept_id}::EQ(from_id))::FIRST
  to_concept <- N<Concept>::WHERE(_::{concept_id}::EQ(to_id))::FIRST
  link <- AddE<CONCEPT_RELATED_TO>({ relation_type: relation_type })::From(from_concept)::To(to_concept)
  RETURN link

QUERY getRelatedConcepts(concept_id: String) =>
  concept <- N<Concept>::WHERE(_::{concept_id}::EQ(concept_id))::FIRST
  related <- concept::Out<CONCEPT_RELATED_TO>
  RETURN related

QUERY addEntityRelation(from_id: String, to_id: String, relationship_type: String, strength: I64, bidirectional: I64) =>
  from_entity <- N<Entity>::WHERE(_::{entity_id}::EQ(from_id))::FIRST
  to_entity <- N<Entity>::WHERE(_::{entity_id}::EQ(to_id))::FIRST
  link <- AddE<RELATES_TO>({ relationship_type: relationship_type, strength: strength, bidirectional: bidirectional })::From(from_entity)::To(to_entity)
  RETURN link

QUERY getEntityRelations(entity_id: String) =>
  entity <- N<Entity>::WHERE(_::{entity_id}::EQ(entity_id))::FIRST
  outgoing <- entity::Out<RELATES_TO>
  incoming <- entity::In<RELATES_TO>
  RETURN outgoing, incoming

QUERY addEntityPartOf(part_id: String, whole_id: String) =>
  part <- N<Entity>::WHERE(_::{entity_id}::EQ(part_id))::FIRST
  whole <- N<Entity>::WHERE(_::{entity_id}::EQ(whole_id))::FIRST
  link <- AddE<PART_OF>::From(part)::To(whole)
  RETURN link

QUERY getEntityParts(entity_id: String) =>
  entity <- N<Entity>::WHERE(_::{entity_id}::EQ(entity_id))::FIRST
  parts <- entity::In<PART_OF>
  RETURN parts

QUERY getEntityWhole(part_id: String) =>
  entity <- N<Entity>::WHERE(_::{entity_id}::EQ(part_id))::FIRST
  whole <- entity::Out<PART_OF>
  RETURN whole

QUERY addMemoryValidIn(memory_id: String, context_id: String, priority: I64, exclusive: I64) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  context <- N<Context>::WHERE(_::{context_id}::EQ(context_id))::FIRST
  link <- AddE<VALID_IN>({ priority: priority, exclusive: exclusive })::From(memory)::To(context)
  RETURN link

QUERY getMemoryValidContexts(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  contexts <- memory::Out<VALID_IN>
  RETURN contexts

QUERY getContextValidMemories(context_id: String) =>
  context <- N<Context>::WHERE(_::{context_id}::EQ(context_id))::FIRST
  memories <- context::In<VALID_IN>
  RETURN memories

QUERY addConstraint(constraint_id: String, rule: String, constraint_type: String, priority: I64, active: I64) =>
  constraint <- AddN<Constraint>({ constraint_id: constraint_id, rule: rule, constraint_type: constraint_type, priority: priority, active: active })
  RETURN constraint

QUERY addMemoryHistoryEvent(memory_id: String, event_id: String, action: String, old_value: String, new_value: String, timestamp: String, actor: String) =>
  event <- AddN<HistoryEvent>({ event_id: event_id, memory_id: memory_id, action: action, old_value: old_value, new_value: new_value, timestamp: timestamp, actor: actor })
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  link <- AddE<HAS_HISTORY>::From(memory)::To(event)
  RETURN event

QUERY getMemoryHistory(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  history <- memory::Out<HAS_HISTORY>
  RETURN history

QUERY addErrorCode(code: String, title: String, description: String, solution: String) =>
  error <- AddN<ErrorCode>({ code: code, title: title, description: description, solution: solution })
  RETURN error

QUERY getConnectionsLevelBatch(memory_ids: [String]) =>
  memories <- N<Memory>::WHERE(_::{memory_id}::IS_IN(memory_ids))
  implies_out_e <- memories::OutE<IMPLIES>
  implies_out_n <- memories::Out<IMPLIES>
  implies_in_e <- memories::InE<IMPLIES>
  implies_in_n <- memories::In<IMPLIES>
  because_out_e <- memories::OutE<BECAUSE>
  because_out_n <- memories::Out<BECAUSE>
  because_in_e <- memories::InE<BECAUSE>
  because_in_n <- memories::In<BECAUSE>
  contradicts_out_e <- memories::OutE<CONTRADICTS>
  contradicts_out_n <- memories::Out<CONTRADICTS>
  contradicts_in_e <- memories::InE<CONTRADICTS>
  contradicts_in_n <- memories::In<CONTRADICTS>
  relation_out_e <- memories::OutE<MEMORY_RELATION>
  relation_out_n <- memories::Out<MEMORY_RELATION>
  relation_in_e <- memories::InE<MEMORY_RELATION>
  relation_in_n <- memories::In<MEMORY_RELATION>
  RETURN memories, implies_out_e, implies_out_n, implies_in_e, implies_in_n, because_out_e, because_out_n, because_in_e, because_in_n, contradicts_out_e, contradicts_out_n, contradicts_in_e, contradicts_in_n, relation_out_e, relation_out_n, relation_in_e, relation_in_n
QUERY linkUserToMemoryWithStance(user_id: String, memory_id: String, context: String, stance: String, certainty: I64, linked_at: String) =>
  user <- N<User>::WHERE(_::{user_id}::EQ(user_id))::FIRST
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  link <- AddE<HAS_MEMORY>({ context: context, access_count: 0, stance: stance, certainty: certainty, linked_at: linked_at, last_confirmed: linked_at })::From(user)::To(memory)
  RETURN link
QUERY getMemoryStances(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  stance_edges <- memory::InE<HAS_MEMORY>
  knowers <- memory::In<HAS_MEMORY>
  RETURN stance_edges, knowers
QUERY enqueuePendingInput(pending_id: String, user_id: String, raw_message: String, agent_id: String, context_tags: String, status: String, created_at: String) =>
  pending <- AddN<PendingInput>({ pending_id: pending_id, user_id: user_id, raw_message: raw_message, agent_id: agent_id, context_tags: context_tags, status: status, created_at: created_at })
  RETURN pending
QUERY getPendingInputsByStatus(status: String, limit: I64) =>
  pending <- N<PendingInput>::WHERE(_::{status}::EQ(status))::RANGE(0, limit)
  RETURN pending
QUERY getPendingInput(pending_id: String) =>
  pending <- N<PendingInput>::WHERE(_::{pending_id}::EQ(pending_id))::FIRST
  RETURN pending
// Compare-and-claim: only one worker may transition a still-pending item.
// The query is atomic inside HelixDB, so additional stdio/gateway processes
// cannot run the same buffered write concurrently.
QUERY claimPendingInput(pending_id: String, expected_status: String, processed_at: String) =>
  pending <- N<PendingInput>::WHERE(AND(_::{pending_id}::EQ(pending_id), _::{status}::EQ(expected_status)))::FIRST
  claimed <- pending::UPDATE({ status: "processing", processed_at: processed_at, result: "", error: "" })
  RETURN claimed
QUERY updatePendingInput(pending_id: String, status: String, processed_at: String, result: String, error: String) =>
  pending <- N<PendingInput>::WHERE(_::{pending_id}::EQ(pending_id))::FIRST
  updated <- pending::UPDATE({ status: status, processed_at: processed_at, result: result, error: error })
  RETURN updated
QUERY deletePendingInput(pending_id: String) =>
  DROP N<PendingInput>::WHERE(_::{pending_id}::EQ(pending_id))
  RETURN "ok"
QUERY enqueueNotice(notice_id: String, user_id: String, kind: String, payload: String, pending_id: String, created_at: String) =>
  notice <- AddN<MemoryNotice>({ notice_id: notice_id, user_id: user_id, kind: kind, payload: payload, pending_id: pending_id, created_at: created_at })
  RETURN notice
QUERY getUndeliveredNotices(user_id: String, limit: I64) =>
  notices <- N<MemoryNotice>::WHERE(AND(_::{user_id}::EQ(user_id), _::{delivered}::EQ(0)))::RANGE(0, limit)
  RETURN notices
QUERY markNoticeDelivered(notice_id: String) =>
  notices <- N<MemoryNotice>::WHERE(_::{notice_id}::EQ(notice_id))
  updated <- notices::UPDATE({ delivered: 1 })
  RETURN updated
// --- Clotho category dictionary queries — Moira #33 (additive) ---
QUERY addCategory(category_id: String, name: String, kind: String, description: String, created_at: String) =>
  category <- AddN<Category>({ category_id: category_id, name: name, kind: kind, description: description, created_at: created_at })
  RETURN category
QUERY getCategoryByName(name: String) =>
  category <- N<Category>::WHERE(_::{name}::EQ(name))::FIRST
  RETURN category
QUERY getAllCategories(limit: I64) =>
  categories <- N<Category>::RANGE(0, limit)
  RETURN categories
QUERY searchSimilarCategories(query_vector: [F64], limit: I64) =>
  embeddings <- SearchV<CategoryEmbedding>(query_vector, limit)
  RETURN embeddings
QUERY linkSubcategory(child_id: String, parent_id: String) =>
  child <- N<Category>::WHERE(_::{category_id}::EQ(child_id))::FIRST
  parent <- N<Category>::WHERE(_::{category_id}::EQ(parent_id))::FIRST
  link <- AddE<SUBCATEGORY_OF>::From(child)::To(parent)
  RETURN link
QUERY addCategoryAlias(alias_id: String, canonical_id: String) =>
  alias <- N<Category>::WHERE(_::{category_id}::EQ(alias_id))::FIRST
  canonical <- N<Category>::WHERE(_::{category_id}::EQ(canonical_id))::FIRST
  link <- AddE<ALIAS_OF>::From(alias)::To(canonical)
  RETURN link
QUERY tagMemoryWithCategory(memory_id: String, category_id: String, confidence: I64, source: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  category <- N<Category>::WHERE(_::{category_id}::EQ(category_id))::FIRST
  link <- AddE<TAGGED_AS>({ confidence: confidence, source: source })::From(memory)::To(category)
  RETURN link
QUERY getMemoryCategories(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  categories <- memory::Out<TAGGED_AS>
  RETURN categories
QUERY getMemoriesByCategory(category_id: String, exclude_memory_id: String, limit: I64) =>
  category <- N<Category>::WHERE(_::{category_id}::EQ(category_id))::FIRST
  memories <- category::In<TAGGED_AS>::WHERE(_::{memory_id}::NEQ(exclude_memory_id))::RANGE(0, limit)
  RETURN memories
QUERY dropConceptByInternalId(concept_internal_id: ID) =>
  DROP N<Concept>(concept_internal_id)
  RETURN "deleted"

QUERY dropCategoryByInternalId(category_internal_id: ID) =>
  DROP N<Category>(category_internal_id)
  RETURN "deleted"

QUERY dropMemoryCascadeByInternalId(memory_internal_id: ID) =>
  DROP N<Memory>(memory_internal_id)::Out<HAS_CHUNK>
  DROP N<Memory>(memory_internal_id)::Out<HAS_EMBEDDING>
  DROP N<Memory>(memory_internal_id)
  RETURN "deleted"

QUERY getCategoryAliases(category_id: String) =>
  category <- N<Category>::WHERE(_::{category_id}::EQ(category_id))::FIRST
  aliases_out <- category::Out<ALIAS_OF>
  aliases_in <- category::In<ALIAS_OF>
  RETURN aliases_out, aliases_in
