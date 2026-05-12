//! Entity domain types: `EntityType`, `EntityEdgeType`, `Entity`, `ExtractedEntity`.
//!
//! These are pure data types with no I/O. The persistence-facing DTO
//! (`EntityDbResponse`) and its conversion into [`Entity`] also live here so
//! the storage shape stays adjacent to the domain shape.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// Wire-format envelope returned by HelixDB queries (`getEntity`, `createEntity`,
/// `getEntityByName`). Kept `pub(super)` so the manager can deserialize into it
/// before mapping into the public [`Entity`].
#[derive(Debug, Deserialize)]
pub(super) struct EntityDbResponse {
    #[serde(default)]
    pub(super) entity_id: String,
    #[serde(default)]
    pub(super) name: String,
    #[serde(default)]
    pub(super) entity_type: String,
    #[serde(default)]
    pub(super) properties: String,
    #[serde(default)]
    pub(super) aliases: String,
}

impl From<EntityDbResponse> for Entity {
    fn from(db: EntityDbResponse) -> Self {
        let entity_type = EntityType::from(db.entity_type.as_str());
        let properties: HashMap<String, serde_json::Value> =
            serde_json::from_str(&db.properties).unwrap_or_default();
        let aliases: Vec<String> = serde_json::from_str(&db.aliases).unwrap_or_default();
        Entity {
            entity_id: db.entity_id,
            name: db.name,
            entity_type,
            properties,
            aliases,
        }
    }
}
