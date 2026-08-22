N::User {
  user_id: String,
  name: String,
  email: String,
  created_at: String,
  metadata: String DEFAULT "{}"
}

// RBAC is persisted in HelixDB, not in a host-local config file.  Assignments
// are immutable-ish audit rows: revocation flips `active` to 0 and preserves
// who granted the role and when.
N::RbacGroup {
  group_id: String,
  name: String,
  description: String DEFAULT "",
  created_at: String,
  active: I64 DEFAULT 1,
  metadata: String DEFAULT "{}"
}
N::RbacDedupGroup {
  dedup_group_id: String,
  name: String,
  description: String DEFAULT "",
  created_at: String,
  active: I64 DEFAULT 1,
  metadata: String DEFAULT "{}"
}
N::RbacAssignment {
  assignment_id: String,
  subject_id: String,
  role: String,
  group_id: String DEFAULT "",
  granted_by: String DEFAULT "",
  created_at: String,
  revoked_at: String DEFAULT "",
  active: I64 DEFAULT 1,
  metadata: String DEFAULT "{}"
}
N::RbacConfig {
  config_id: String,
  enabled: I64 DEFAULT 0,
  migration_state: String DEFAULT "pending",
  migration_kind: String DEFAULT "",
  updated_at: String,
  updated_by: String DEFAULT ""
}
E::RBAC_MEMBER_OF {
  From: User,
  To: RbacGroup,
  Properties: {
    assignment_id: String,
    role: String,
    granted_by: String,
    granted_at: String,
    active: I64
  }
}
E::MEMORY_IN_RBAC_GROUP {
  From: Memory,
  To: RbacGroup,
  Properties: {
    assigned_by: String,
    assigned_at: String
  }
}
E::RBAC_GROUP_IN_DEDUP_GROUP {
  From: RbacGroup,
  To: RbacDedupGroup,
  Properties: {
    assigned_by: String,
    assigned_at: String,
    removed_at: String,
    active: I64
  }
}
E::MEMORY_IN_RBAC_DEDUP_GROUP {
  From: Memory,
  To: RbacDedupGroup,
  Properties: {
    assigned_by: String,
    assigned_at: String
  }
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
  principal_id: String DEFAULT "",
  name: String,
  role: String,
  capabilities: String,
  agent_version: String,
  created_at: String,
  host: String DEFAULT "",
  last_seen: String DEFAULT "",
  status: String DEFAULT "idle"
}
N::Memory {
  memory_id: String,
  INDEX content_key: String DEFAULT "",
  rbac_scope: String DEFAULT "",
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
  deleted_by: String DEFAULT "",
  user_count: I64 DEFAULT 1
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
    access_count: I64,
    stance: String,
    certainty: I64,
    linked_at: String,
    last_confirmed: String
  }
}
E::INSTANCE_OF {
  From: Memory,
  To: Concept,
  Properties: {
    confidence: I64
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
E::CHUNK_TO_EMBEDDING {
    From: DocChunk,
    To: ChunkEmbedding,
    Properties: {}
}
E::CONCEPT_RELATED_TO {
    From: Concept,
    To: Concept,
    Properties: {
        relation_type: String
    }
}
V::ChunkEmbedding {
    embedding: [F64]
}
V::ConceptEmbedding {
    embedding: [F64]
}

N::PendingInput {
  pending_id: String,
  user_id: String DEFAULT "",
  actor_id: String DEFAULT "",
  group_id: String DEFAULT "",
  raw_message: String,
  agent_id: String DEFAULT "",
  context_tags: String DEFAULT "",
  status: String DEFAULT "pending",
  created_at: String DEFAULT "{{timestamp}}",
  processed_at: String DEFAULT "",
  result: String DEFAULT "",
  error: String DEFAULT ""
}

N::MemoryNotice {
  notice_id: String,
  user_id: String DEFAULT "",
  kind: String DEFAULT "add_result",
  payload: String DEFAULT "{}",
  pending_id: String DEFAULT "",
  created_at: String DEFAULT "{{timestamp}}",
  delivered: I64 DEFAULT 0
}

// --- Clotho category dictionary (controlled vocabulary) — Moira #33 ---
N::Category {
  category_id: String,
  name: String,
  kind: String,
  description: String,
  created_at: String
}
V::CategoryEmbedding {
  name: String
}
E::SUBCATEGORY_OF {
  From: Category,
  To: Category
}
E::ALIAS_OF {
  From: Category,
  To: Category
}
E::TAGGED_AS {
  From: Memory,
  To: Category,
  Properties: {
    confidence: I64,
    source: String
  }
}
// Admin-only provenance for the Moirai memory layer. Standard reasoning and
// smart-traversal queries intentionally do not walk this edge family.
E::MOIRAI_DERIVED_FROM {
  From: Memory,
  To: Memory,
  Properties: {
    source: String,
    created_at: String
  }
}
