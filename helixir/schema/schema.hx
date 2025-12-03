N::User {
  user_id: String,
  name: String,
  email: String,
  created_at: String,
  metadata: String DEFAULT "{}"
}
N::Session {
  session_id: String,
  started_at: String,
  ended_at: String,
  status: String,
  session_type: String,
  metadata: String DEFAULT "{}"
}
N::Agent {
  agent_id: String,
  name: String,
  role: String,
  capabilities: String,
  version: String,
  created_at: String
}
E::IN_SESSION {
  From: User,
  To: Session,
  Properties: {
    role: String
  }
}
N::Memory {
  memory_id: String,
  user_id: String DEFAULT "",
  content: String,
  memory_type: String DEFAULT "fact",
  certainty: I64 DEFAULT 100,
  importance: I64 DEFAULT 50,
  created_at: String DEFAULT "{{timestamp}}",
  updated_at: String DEFAULT "{{timestamp}}",
  valid_from: String DEFAULT "{{timestamp}}",
  valid_until: String DEFAULT "",
  immutable: I64 DEFAULT 0,
  verified: I64 DEFAULT 0,
  context_tags: String DEFAULT "",
  source: String DEFAULT "manual",
  metadata: String DEFAULT "{}",
  is_deleted: I64 DEFAULT 0,
  deleted_at: String DEFAULT "",
  deleted_by: String DEFAULT ""
}
N::Entity {
  entity_id: String,
  name: String,
  entity_type: String,
  properties: String,
  aliases: String
}
N::Concept {
  concept_id: String,
  name: String,
  level: I64,
  description: String,
  parent_id: String,
  properties: String
}
E::HAS_MEMORY {
  From: User,
  To: Memory,
  Properties: {
    context: String,
    access_count: I64
  }
}
E::INSTANCE_OF {
  From: Memory,
  To: Concept,
  Properties: {
    confidence: I64
  }
}
E::BELONGS_TO_CATEGORY {
  From: Memory,
  To: Concept,
  Properties: {
    relevance: I64
  }
}
E::MENTIONS {
  From: Memory,
  To: Entity,
  Properties: {
    salience: I64,
    sentiment: String
  }
}
E::EXTRACTED_ENTITY {
  From: Memory,
  To: Entity,
  Properties: {
    confidence: I64,
    method: String
  }
}
E::IS_A {
  From: Concept,
  To: Concept,
  Properties: {
    inheritance_type: String
  }
}
E::HAS_SUBTYPE {
  From: Concept,
  To: Concept,
  Properties: {}
}
E::RELATES_TO {
  From: Entity,
  To: Entity,
  Properties: {
    relationship_type: String,
    strength: I64,
    bidirectional: I64
  }
}
E::PART_OF {
  From: Entity,
  To: Entity,
  Properties: {}
}
N::Context {
  context_id: String,
  name: String,
  context_type: String,
  properties: String,
  parent_context: String
}
N::Constraint {
  constraint_id: String,
  rule: String,
  constraint_type: String,
  priority: I64,
  active: I64
}
N::Reasoning {
  reasoning_id: String,
  reasoning_type: String,
  description: String,
  confidence: I64,
  created_at: String
}
N::HistoryEvent {
  event_id: String,
  memory_id: String,
  action: String,
  old_value: String,
  new_value: String,
  timestamp: String,
  actor: String
}
E::VALID_IN {
  From: Memory,
  To: Context,
  Properties: {
    priority: I64,
    exclusive: I64
  }
}
E::APPLIES_IN {
  From: Constraint,
  To: Context,
  Properties: {}
}
E::OCCURRED_IN {
  From: Memory,
  To: Context,
  Properties: {
    timestamp: String
  }
}
E::CREATED_IN {
  From: Memory,
  To: Session,
  Properties: {
    sequence: I64
  }
}
E::AGENT_CREATED {
  From: Agent,
  To: Memory,
  Properties: {
    timestamp: String,
    method: String
  }
}
E::HAS_HISTORY {
  From: Memory,
  To: HistoryEvent,
  Properties: {}
}
N::MemoryChunk {
  chunk_id: String,
  position: I64,
  parent_memory_id: String,
  content: String,
  token_count: I64,
  created_at: String DEFAULT "{{timestamp}}"
}
E::HAS_CHUNK {
  From: Memory,
  To: MemoryChunk,
  Properties: {
    chunk_index: I64
  }
}
E::NEXT_CHUNK {
  From: MemoryChunk,
  To: MemoryChunk,
  Properties: {}
}
E::CHUNK_HAS_EMBEDDING {
  From: MemoryChunk,
  To: MemoryEmbedding,
  Properties: {
    embedding_model: String
  }
}
E::MEMORY_RELATION {
  From: Memory,
  To: Memory,
  Properties: {
    relation_type: String,
    strength: I64,
    created_at: String,
    metadata: String
  }
}
E::IMPLIES {
  From: Memory,
  To: Memory,
  Properties: {
    probability: I64,
    reasoning_id: String
  }
}
E::BECAUSE {
  From: Memory,
  To: Memory,
  Properties: {
    strength: I64,
    reasoning_id: String
  }
}
E::CONTRADICTS {
  From: Memory,
  To: Memory,
  Properties: {
    resolution: String,
    resolved: I64,
    resolution_strategy: String
  }
}
E::SUPERSEDES {
  From: Memory,
  To: Memory,
  Properties: {
    reason: String,
    superseded_at: String,
    is_contradiction: I64
  }
}
V::MemoryEmbedding {
  content: String,
  created_at: Date
}
V::EntityEmbedding {
  name: String
}
E::HAS_EMBEDDING {
  From: Memory,
  To: MemoryEmbedding,
  Properties: {
    embedding_model: String
  }
}
E::ENTITY_HAS_EMBEDDING {
  From: Entity,
  To: EntityEmbedding,
  Properties: {
    embedding_model: String
  }
}
N::DocPage {
    INDEX url: String,
    title: String,
    category: String,
    word_count: U32,
    created_at: Date DEFAULT NOW
}
N::DocChunk {
    INDEX chunk_id: String,
    content: String,
    chunk_index: U32,
    word_count: U32,
    section_title: String,
    created_at: Date DEFAULT NOW
}
N::CodeExample {
    INDEX example_id: String,
    code: String,
    language: String,
    description: String,
    created_at: Date DEFAULT NOW
}
N::ErrorCode {
    INDEX code: String,
    title: String,
    description: String,
    solution: String,
    created_at: Date DEFAULT NOW
}
E::PAGE_TO_CHUNK {
    From: DocPage,
    To: DocChunk,
    Properties: {}
}
E::CHUNK_TO_EMBEDDING {
    From: DocChunk,
    To: ChunkEmbedding,
    Properties: {}
}
E::CHUNK_MENTIONS_CONCEPT {
    From: DocChunk,
    To: Concept,
    Properties: {
        relevance_score: F64
    }
}
E::CONCEPT_RELATED_TO {
    From: Concept,
    To: Concept,
    Properties: {
        relation_type: String
    }
}
E::CHUNK_HAS_EXAMPLE {
    From: DocChunk,
    To: CodeExample,
    Properties: {}
}
E::CONCEPT_HAS_EXAMPLE {
    From: Concept,
    To: CodeExample,
    Properties: {}
}
E::ERROR_REFERENCES_CONCEPT {
    From: ErrorCode,
    To: Concept,
    Properties: {}
}
V::ChunkEmbedding {
    embedding: [F64]
}
V::ConceptEmbedding {
    embedding: [F64]
}
