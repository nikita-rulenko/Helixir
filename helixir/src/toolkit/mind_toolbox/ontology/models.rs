use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use strum::{EnumString, IntoStaticStr};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ConceptType {
    Abstract,
    Concrete,
}

#[derive(Debug, Clone, Serialize, Deserialize, EnumString, IntoStaticStr, PartialEq, Eq)]
pub enum RelationType {
    #[strum(serialize = "IS_A")]
    IsA,
    #[strum(serialize = "HAS_SUBTYPE")]
    HasSubtype,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Concept {
    pub concept_id: String,
    pub name: String,
    pub concept_type: ConceptType,
    pub description: String,
    pub parent_concept: Option<String>,
    pub level: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptRelation {
    pub from_concept: String,
    pub to_concept: String,
    pub relation_type: RelationType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyStats {
    pub total_concepts: usize,
    pub total_relations: usize,
    pub concepts_by_type: HashMap<String, usize>,
    pub max_depth: usize,
}

impl Concept {
    pub fn new(
        concept_id: String,
        name: String,
        concept_type: ConceptType,
        description: String,
        parent_concept: Option<String>,
        level: u8,
    ) -> Self {
        Self {
            concept_id,
            name,
            concept_type,
            description,
            parent_concept,
            level,
        }
    }
}

impl ConceptRelation {
    pub fn new(
        from_concept: String,
        to_concept: String,
        relation_type: RelationType,
    ) -> Self {
        Self {
            from_concept,
            to_concept,
            relation_type,
        }
    }
}