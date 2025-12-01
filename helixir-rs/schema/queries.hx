QUERY addUser(user_id: String, name: String) =>
  user <- AddN<User>({ user_id: user_id, name: name })
  RETURN user
QUERY getUser(user_id: String) =>
  user <- N<User>::WHERE(_::{user_id}::EQ(user_id))::FIRST
  RETURN user
QUERY addMemory(memory_id: String, user_id: String, content: String, memory_type: String, certainty: I64, importance: I64, created_at: String, updated_at: String, context_tags: String, source: String, metadata: String) =>
  memory <- AddN<Memory>({ memory_id: memory_id, user_id: user_id, content: content, memory_type: memory_type, certainty: certainty, importance: importance, created_at: created_at, updated_at: updated_at, context_tags: context_tags, source: source, metadata: metadata })
  RETURN memory
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
QUERY addMemorySupersession(new_id: String, old_id: String, reason: String, superseded_at: String, is_contradiction: I64) =>
  new_memory <- N<Memory>::WHERE(_::{memory_id}::EQ(new_id))::FIRST
  old_memory <- N<Memory>::WHERE(_::{memory_id}::EQ(old_id))::FIRST
  supersedes <- AddE<SUPERSEDES>({ reason: reason, superseded_at: superseded_at, is_contradiction: is_contradiction })::From(new_memory)::To(old_memory)
  RETURN supersedes
QUERY getSupersededMemories(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  superseded <- memory::Out<SUPERSEDES>
  RETURN superseded
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
QUERY addMemoryToContext(memory_id: String, context_id: String, timestamp: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  context <- N<Context>::WHERE(_::{context_id}::EQ(context_id))::FIRST
  link <- AddE<OCCURRED_IN>({ timestamp: timestamp })::From(memory)::To(context)
  RETURN link
QUERY getMemoryContext(memory_id: ID) =>
  memory <- N<Memory>(memory_id)
  context <- memory::Out<OCCURRED_IN>
  RETURN context
QUERY getContextMemories(context_id: ID) =>
  context <- N<Context>(context_id)
  memories <- context::In<OCCURRED_IN>
  RETURN memories
QUERY addMemoryEmbedding(memory_id: ID, vector_data: [F64], embedding_model: String, created_at: Date) =>
  embedding <- AddV<MemoryEmbedding>(vector_data, { created_at: created_at })
  link <- AddE<HAS_EMBEDDING>({ embedding_model: embedding_model })::From(memory_id)::To(embedding)
  RETURN embedding
QUERY getMemoryByEmbeddingId(embedding_id: ID) =>
  embedding <- V<MemoryEmbedding>(embedding_id)
  memory <- embedding::In<HAS_EMBEDDING>
  RETURN memory
QUERY addEntityEmbedding(entity_id: ID, vector_data: [F64], content: String, embedding_model: String) =>
  embedding <- AddV<EntityEmbedding>(vector_data, { name: content })
  link <- AddE<ENTITY_HAS_EMBEDDING>({ embedding_model: embedding_model })::From(entity_id)::To(embedding)
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
QUERY linkMemoryToCategory(memory_id: String, concept_id: String, relevance: I64) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  concept <- N<Concept>::WHERE(_::{concept_id}::EQ(concept_id))::FIRST
  link <- AddE<BELONGS_TO_CATEGORY>({ relevance: relevance })::From(memory)::To(concept)
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
QUERY searchSimilarEntities(query_vector: [F64], limit: I64) =>
  embeddings <- SearchV<EntityEmbedding>(query_vector, limit)
  RETURN embeddings
QUERY searchMemoriesByContext(query_vector: [F64], context_id: ID, limit: I64) =>
  context <- N<Context>(context_id)
  context_memories <- context::In<OCCURRED_IN>
  embeddings <- SearchV<MemoryEmbedding>(query_vector, limit)
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
QUERY addDocChunk(chunk_id: String, content: String, chunk_index: I64, word_count: I64, section_title: String, page_url: String) =>
  chunk <- AddN<DocChunk>({ chunk_id: chunk_id, content: content, chunk_index: chunk_index, word_count: word_count, section_title: section_title })
  page <- N<DocPage>::WHERE(_::{url}::EQ(page_url))::FIRST
  link <- AddE<PAGE_TO_CHUNK>::From(page)::To(chunk)
  RETURN chunk
QUERY addChunkEmbedding(chunk_id: String, vector_data: [F64]) =>
  chunk <- N<DocChunk>::WHERE(_::{chunk_id}::EQ(chunk_id))::FIRST
  embedding <- AddV<ChunkEmbedding>(vector_data)
  link <- AddE<CHUNK_TO_EMBEDDING>::From(chunk)::To(embedding)
  RETURN embedding
QUERY searchDocChunks(query_vector: [F64], limit: I64) =>
  embeddings <- SearchV<ChunkEmbedding>(query_vector, limit)
  RETURN embeddings
QUERY getDocChunksByPage(page_url: String) =>
  page <- N<DocPage>::WHERE(_::{url}::EQ(page_url))::FIRST
  chunks <- page::Out<PAGE_TO_CHUNK>
  RETURN chunks
QUERY addCodeExample(example_id: String, code: String, language: String, description: String) =>
  example <- AddN<CodeExample>({ example_id: example_id, code: code, language: language, description: description })
  RETURN example
QUERY linkChunkToExample(chunk_id: String, example_id: String) =>
  chunk <- N<DocChunk>::WHERE(_::{chunk_id}::EQ(chunk_id))::FIRST
  example <- N<CodeExample>::WHERE(_::{example_id}::EQ(example_id))::FIRST
  link <- AddE<CHUNK_HAS_EXAMPLE>::From(chunk)::To(example)
  RETURN link
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

QUERY linkChunkToNext(from_chunk_id: String, to_chunk_id: String) =>
  from_chunk <- N<MemoryChunk>::WHERE(_::{chunk_id}::EQ(from_chunk_id))::FIRST
  to_chunk <- N<MemoryChunk>::WHERE(_::{chunk_id}::EQ(to_chunk_id))::FIRST
  link <- AddE<NEXT_CHUNK>::From(from_chunk)::To(to_chunk)
  RETURN link

QUERY getMemoryChunks(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  chunks <- memory::Out<HAS_CHUNK>
  RETURN chunks

QUERY getMemoryWithChunks(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  chunks <- memory::Out<HAS_CHUNK>
  RETURN memory, chunks

QUERY addChunkEmbeddingByID(chunk_internal_id: ID, vector_data: [F64], content: String, embedding_model: String, created_at: Date) =>
  embedding <- AddV<MemoryEmbedding>(vector_data, { content: content, created_at: created_at })
  link <- AddE<CHUNK_HAS_EMBEDDING>({ embedding_model: embedding_model })::From(chunk_internal_id)::To(embedding)
  RETURN embedding

QUERY getUserMemories(user_id: String, limit: I64) =>
  user <- N<User>::WHERE(_::{user_id}::EQ(user_id))::FIRST
  memories <- user::Out<HAS_MEMORY>::RANGE(0, limit)
  RETURN memories

QUERY getMemoryEntities(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  entities <- memory::Out<EXTRACTED_ENTITY>
  mentions <- memory::Out<MENTIONS>
  RETURN entities, mentions

QUERY getMemoryConcepts(memory_id: String) =>
  memory <- N<Memory>::WHERE(_::{memory_id}::EQ(memory_id))::FIRST
  instance_of <- memory::Out<INSTANCE_OF>
  belongs_to <- memory::Out<BELONGS_TO_CATEGORY>
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
  categories <- memory::Out<BELONGS_TO_CATEGORY>
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
