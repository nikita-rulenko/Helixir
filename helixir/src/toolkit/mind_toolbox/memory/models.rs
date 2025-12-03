use chrono::Utc;
use serde::{Deserialize, Serialize};
use strum::{EnumString, IntoStaticStr};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum EntityType {
    Person,
    Organization,
    Location,
    System,
    Component,
    Resource,
    Concept,
    Process,
    Event,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    
    pub memory_id: String,
    pub content: String,
    pub memory_type: String,
    pub user_id: String,
    
    
    pub certainty: i64,      
    pub importance: i64,     
    
    
    pub created_at: String,
    pub updated_at: String,
    pub valid_from: String,
    pub valid_until: String,  
    
    
    pub immutable: i64,
    pub verified: i64,
    
    
    pub context_tags: String,
    pub source: String,
    pub metadata: String,
    
    
    pub is_deleted: i64,
    pub deleted_at: String,
    pub deleted_by: String,
    
    
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub concepts: Vec<String>,
}

impl Memory {
    pub fn builder() -> MemoryBuilder {
        MemoryBuilder::default()
    }
}

#[derive(Default)]
pub struct MemoryBuilder {
    memory_id: Option<String>,
    content: Option<String>,
    memory_type: Option<String>,
    user_id: Option<String>,
    certainty: Option<i64>,
    importance: Option<i64>,
    created_at: Option<String>,
    updated_at: Option<String>,
    valid_from: Option<String>,
    valid_until: Option<String>,
    immutable: Option<i64>,
    verified: Option<i64>,
    context_tags: Option<String>,
    source: Option<String>,
    metadata: Option<String>,
    is_deleted: Option<i64>,
    deleted_at: Option<String>,
    deleted_by: Option<String>,
    concepts: Option<Vec<String>>,
}

impl MemoryBuilder {
    pub fn memory_id(mut self, memory_id: String) -> Self {
        self.memory_id = Some(memory_id);
        self
    }

    pub fn content(mut self, content: String) -> Self {
        self.content = Some(content);
        self
    }

    pub fn memory_type(mut self, memory_type: String) -> Self {
        self.memory_type = Some(memory_type);
        self
    }

    pub fn user_id(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn certainty(mut self, certainty: i64) -> Self {
        self.certainty = Some(certainty);
        self
    }

    pub fn importance(mut self, importance: i64) -> Self {
        self.importance = Some(importance);
        self
    }

    pub fn created_at(mut self, created_at: String) -> Self {
        self.created_at = Some(created_at);
        self
    }

    pub fn updated_at(mut self, updated_at: String) -> Self {
        self.updated_at = Some(updated_at);
        self
    }

    pub fn valid_from(mut self, valid_from: String) -> Self {
        self.valid_from = Some(valid_from);
        self
    }

    pub fn valid_until(mut self, valid_until: String) -> Self {
        self.valid_until = Some(valid_until);
        self
    }

    pub fn immutable(mut self, immutable: i64) -> Self {
        self.immutable = Some(immutable);
        self
    }

    pub fn verified(mut self, verified: i64) -> Self {
        self.verified = Some(verified);
        self
    }

    pub fn context_tags(mut self, context_tags: String) -> Self {
        self.context_tags = Some(context_tags);
        self
    }

    pub fn source(mut self, source: String) -> Self {
        self.source = Some(source);
        self
    }

    pub fn metadata(mut self, metadata: String) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn is_deleted(mut self, is_deleted: i64) -> Self {
        self.is_deleted = Some(is_deleted);
        self
    }

    pub fn deleted_at(mut self, deleted_at: String) -> Self {
        self.deleted_at = Some(deleted_at);
        self
    }

    pub fn deleted_by(mut self, deleted_by: String) -> Self {
        self.deleted_by = Some(deleted_by);
        self
    }

    pub fn concepts(mut self, concepts: Vec<String>) -> Self {
        self.concepts = Some(concepts);
        self
    }

    pub fn build(self) -> Memory {
        let now = Utc::now().to_rfc3339();
        Memory {
            memory_id: self.memory_id.unwrap_or_else(|| format!("mem_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..12].to_string())),
            content: self.content.unwrap_or_default(),
            memory_type: self.memory_type.unwrap_or_else(|| "fact".to_string()),
            user_id: self.user_id.unwrap_or_default(),
            certainty: self.certainty.unwrap_or(100),
            importance: self.importance.unwrap_or(50),
            created_at: self.created_at.unwrap_or_else(|| now.clone()),
            updated_at: self.updated_at.unwrap_or_else(|| now.clone()),
            valid_from: self.valid_from.unwrap_or_else(|| now.clone()),
            valid_until: self.valid_until.unwrap_or_default(),
            immutable: self.immutable.unwrap_or(0),
            verified: self.verified.unwrap_or(0),
            context_tags: self.context_tags.unwrap_or_default(),
            source: self.source.unwrap_or_else(|| "manual".to_string()),
            metadata: self.metadata.unwrap_or_else(|| "{}".to_string()),
            is_deleted: self.is_deleted.unwrap_or(0),
            deleted_at: self.deleted_at.unwrap_or_default(),
            deleted_by: self.deleted_by.unwrap_or_default(),
            concepts: self.concepts.unwrap_or_default(),
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub entity_id: String,
    pub name: String,
    pub entity_type: String,  
    pub properties: String,   
    pub aliases: String,      
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    pub context_id: String,
    pub name: String,
    pub context_type: String,
    pub properties: String,      
    pub parent_context: String,  
}


#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryStats {
    pub total_memories: i64,
    pub memories_by_type: HashMap<String, i64>,
    pub memories_by_user: HashMap<String, i64>,
    pub avg_certainty: f64,
    pub avg_importance: f64,
    pub oldest_memory: Option<String>,
    pub newest_memory: Option<String>,
}