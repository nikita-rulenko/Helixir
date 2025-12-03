

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::db::HelixClient;


#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntityType {
    Person,
    Organization,
    Location,
    Technology,
    Concept,
    Event,
    Product,
    System,
    Component,
    Resource,
    Process,
    
    Custom(String),
}

impl Default for EntityType {
    fn default() -> Self {
        Self::Concept
    }
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Person => write!(f, "person"),
            Self::Organization => write!(f, "organization"),
            Self::Location => write!(f, "location"),
            Self::Technology => write!(f, "technology"),
            Self::Concept => write!(f, "concept"),
            Self::Event => write!(f, "event"),
            Self::Product => write!(f, "product"),
            Self::System => write!(f, "system"),
            Self::Component => write!(f, "component"),
            Self::Resource => write!(f, "resource"),
            Self::Process => write!(f, "process"),
            Self::Custom(s) => write!(f, "{}", s),
        }
    }
}

impl From<&str> for EntityType {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "person" => Self::Person,
            "organization" => Self::Organization,
            "location" => Self::Location,
            "technology" => Self::Technology,
            "concept" => Self::Concept,
            "event" => Self::Event,
            "product" => Self::Product,
            "system" => Self::System,
            "component" => Self::Component,
            "resource" => Self::Resource,
            "process" => Self::Process,
            other => Self::Custom(other.to_string()),
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityEdgeType {
    
    ExtractedEntity,
    
    Mentions,
}

impl std::fmt::Display for EntityEdgeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExtractedEntity => write!(f, "EXTRACTED_ENTITY"),
            Self::Mentions => write!(f, "MENTIONS"),
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub entity_id: String,
    pub name: String,
    pub entity_type: EntityType,
    pub properties: HashMap<String, serde_json::Value>,
    pub aliases: Vec<String>,
}


#[derive(Debug, Deserialize)]
struct EntityDbResponse {
    #[serde(default)]
    entity_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    entity_type: String,
    #[serde(default)]
    properties: String,
    #[serde(default)]
    aliases: String,
}

impl From<EntityDbResponse> for Entity {
    fn from(db: EntityDbResponse) -> Self {
        let entity_type = EntityType::from(db.entity_type.as_str());
        let properties: HashMap<String, serde_json::Value> = 
            serde_json::from_str(&db.properties).unwrap_or_default();
        let aliases: Vec<String> = 
            serde_json::from_str(&db.aliases).unwrap_or_default();
        Entity {
            entity_id: db.entity_id,
            name: db.name,
            entity_type,
            properties,
            aliases,
        }
    }
}

impl Entity {
    
    pub fn new(name: String, entity_type: EntityType) -> Self {
        let entity_id = format!(
            "ent_{}",
            uuid::Uuid::new_v4()
                .to_string()
                .replace("-", "")
                .chars()
                .take(12)
                .collect::<String>()
        );
        Self {
            entity_id,
            name,
            entity_type,
            properties: HashMap::new(),
            aliases: Vec::new(),
        }
    }

    
    pub fn with_id(entity_id: String, name: String, entity_type: EntityType) -> Self {
        Self {
            entity_id,
            name,
            entity_type,
            properties: HashMap::new(),
            aliases: Vec::new(),
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    pub name: String,
    pub entity_type: String,
    pub confidence: i32,
}


#[derive(Error, Debug)]
pub enum EntityError {
    #[error("Entity not found: {0}")]
    NotFound(String),
    #[error("Entity already exists: {0}")]
    AlreadyExists(String),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Invalid entity: {0}")]
    Invalid(String),
    #[error("Validation error: {0}")]
    Validation(String),
}


pub struct EntityManager {
    client: Arc<HelixClient>,
    
    entity_cache: RwLock<HashMap<String, Entity>>,
    
    name_to_id: RwLock<HashMap<String, String>>,
    cache_size: usize,
}

impl EntityManager {
    
    pub fn new(client: Arc<HelixClient>, cache_size: usize) -> Self {
        info!("EntityManager initialized (cache_size={})", cache_size);
        Self {
            client,
            entity_cache: RwLock::new(HashMap::new()),
            name_to_id: RwLock::new(HashMap::new()),
            cache_size,
        }
    }

    
    fn add_to_cache(&self, entity: &Entity) {
        let mut cache = self.entity_cache.write();
        let mut name_map = self.name_to_id.write();

        
        if cache.len() >= self.cache_size {
            if let Some(oldest_id) = cache.keys().next().cloned() {
                if let Some(evicted) = cache.remove(&oldest_id) {
                    name_map.remove(&evicted.name.to_lowercase());
                    debug!("Cache eviction: {} (size: {})", oldest_id, self.cache_size);
                }
            }
        }

        cache.insert(entity.entity_id.clone(), entity.clone());
        name_map.insert(entity.name.to_lowercase(), entity.entity_id.clone());
    }

    
    pub async fn create_entity(
        &self,
        name: &str,
        entity_type: &str,
        properties: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<Entity, EntityError> {
        
        let name = name.trim();
        if name.is_empty() {
            return Err(EntityError::Validation("Entity name cannot be empty".into()));
        }

        let entity_type = EntityType::from(entity_type);
        let entity = Entity::new(name.to_string(), entity_type);
        let mut entity = entity;
        if let Some(props) = properties {
            entity.properties = props;
        }

        
        #[derive(Deserialize)]
        struct CreateEntityResponse {
            entity: EntityDbResponse,
        }
        
        match self
            .client
            .execute_query::<CreateEntityResponse, _>(
                "createEntity",
                &serde_json::json!({
                    "entity_id": entity.entity_id,
                    "name": entity.name,
                    "entity_type": entity.entity_type.to_string(),
                    "properties": serde_json::to_string(&entity.properties).unwrap_or_default(),
                    "aliases": "[]",
                }),
            )
            .await
        {
            Ok(_) => {
                info!(
                    "Created entity in DB and cache: {} ({})",
                    entity.name, entity.entity_type
                );
            }
            Err(e) => {
                warn!(
                    "Failed to persist entity to HelixDB: {}, adding to cache only",
                    e
                );
            }
        }

        
        self.add_to_cache(&entity);

        Ok(entity)
    }

    
    pub async fn get_entity(&self, entity_id: &str) -> Result<Option<Entity>, EntityError> {
        
        {
            let cache = self.entity_cache.read();
            if let Some(entity) = cache.get(entity_id) {
                debug!("Cache HIT: {}", entity_id);
                return Ok(Some(entity.clone()));
            }
        }

        
        debug!("Cache MISS: {}, querying HelixDB", entity_id);

        #[derive(Deserialize)]
        struct EntityResult {
            entity: Option<EntityDbResponse>,
        }

        match self
            .client
            .execute_query::<EntityResult, _>("getEntity", &serde_json::json!({"entity_id": entity_id}))
            .await
        {
            Ok(result) => {
                if let Some(db_entity) = result.entity {
                    let entity: Entity = db_entity.into();
                    self.add_to_cache(&entity);
                    debug!("Loaded from DB and cached: {}", entity_id);
                    return Ok(Some(entity));
                }
                Ok(None)
            }
            Err(e) => {
                warn!("Failed to query HelixDB for entity {}: {}", entity_id, e);
                Ok(None)
            }
        }
    }

    
    pub async fn get_or_create_entity(
        &self,
        name: &str,
        entity_type: &str,
        properties: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<Entity, EntityError> {
        let normalized_name = name.trim().to_lowercase();

        
        {
            let name_map = self.name_to_id.read();
            if let Some(entity_id) = name_map.get(&normalized_name) {
                let cache = self.entity_cache.read();
                if let Some(entity) = cache.get(entity_id) {
                    debug!("Entity found in cache: {}", name);
                    return Ok(entity.clone());
                }
            }
        }

        
        #[derive(Deserialize)]
        struct EntityByNameResult {
            entity: Option<EntityDbResponse>,
        }

        match self
            .client
            .execute_query::<EntityByNameResult, _>("getEntityByName", &serde_json::json!({"name": name}))
            .await
        {
            Ok(result) => {
                if let Some(db_entity) = result.entity {
                    let entity: Entity = db_entity.into();
                    self.add_to_cache(&entity);
                    debug!("Entity found in DB: {}", name);
                    return Ok(entity);
                }
            }
            Err(e) => {
                debug!("Entity not found in DB: {} ({})", name, e);
            }
        }

        
        debug!("Creating new entity: {}", name);
        self.create_entity(name, entity_type, properties).await
    }

    
    pub async fn link_to_memory(
        &self,
        entity_id: &str,
        memory_id: &str,
        edge_type: EntityEdgeType,
        confidence: i32,
        salience: i32,
        sentiment: &str,
    ) -> Result<(), EntityError> {
        
        #[derive(Deserialize)]
        struct EdgeResponse {
            #[serde(default)]
            link: serde_json::Value,
        }
        
        match edge_type {
            EntityEdgeType::ExtractedEntity => {
                self.client
                    .execute_query::<EdgeResponse, _>(
                        "linkExtractedEntity",
                        &serde_json::json!({
                            "memory_id": memory_id,
                            "entity_id": entity_id,
                            "confidence": confidence as i64,
                            "method": "llm",
                        }),
                    )
                    .await
                    .map_err(|e| EntityError::Database(e.to_string()))?;
            }
            EntityEdgeType::Mentions => {
                self.client
                    .execute_query::<EdgeResponse, _>(
                        "linkMentionsEntity",
                        &serde_json::json!({
                            "memory_id": memory_id,
                            "entity_id": entity_id,
                            "salience": salience as i64,
                            "sentiment": sentiment,
                        }),
                    )
                    .await
                    .map_err(|e| EntityError::Database(e.to_string()))?;
            }
        }

        info!(
            "Linked entity {}.. to memory {}.. ({})",
            crate::safe_truncate(entity_id, 8),
            crate::safe_truncate(memory_id, 8),
            edge_type
        );

        Ok(())
    }

    
    pub async fn get_entities_for_memory(&self, memory_id: &str) -> Result<Vec<Entity>, EntityError> {
        #[derive(Deserialize)]
        struct EntitiesResult {
            entities: Vec<Entity>,
        }

        match self
            .client
            .execute_query::<EntitiesResult, _>(
                "getEntitiesForMemory",
                &serde_json::json!({"memory_id": memory_id}),
            )
            .await
        {
            Ok(result) => {
                
                for entity in &result.entities {
                    self.add_to_cache(entity);
                }
                debug!(
                    "Found {} entities for memory {}",
                    result.entities.len(),
                    crate::safe_truncate(memory_id, 8)
                );
                Ok(result.entities)
            }
            Err(e) => {
                warn!("Failed to get entities for memory {}: {}", memory_id, e);
                Ok(Vec::new())
            }
        }
    }

    
    pub async fn search_entities(&self, query: &str, limit: usize) -> Result<Vec<Entity>, EntityError> {
        #[derive(Deserialize)]
        struct EntitiesResult {
            entities: Vec<Entity>,
        }

        match self
            .client
            .execute_query::<EntitiesResult, _>(
                "searchEntities",
                &serde_json::json!({"query": query, "limit": limit}),
            )
            .await
        {
            Ok(result) => {
                for entity in &result.entities {
                    self.add_to_cache(entity);
                }
                Ok(result.entities)
            }
            Err(e) => {
                warn!("Failed to search entities: {}", e);
                Ok(Vec::new())
            }
        }
    }

    
    pub fn cache_stats(&self) -> (usize, usize) {
        let cache = self.entity_cache.read();
        let name_map = self.name_to_id.read();
        (cache.len(), name_map.len())
    }
}

impl std::fmt::Debug for EntityManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (cached, names) = self.cache_stats();
        write!(
            f,
            "EntityManager(cached_entities={}, name_mappings={})",
            cached, names
        )
    }
}


pub use EntityEdgeType as EdgeType;
