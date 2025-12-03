use crate::db::HelixClient;
use std::sync::Arc;
use std::collections::HashMap;
use super::models::{Concept, ConceptType, ConceptRelation, RelationType};
use tracing::{debug, info, warn};
use thiserror::Error;
use serde::{Serialize, Deserialize};

#[derive(Debug, Error)]
pub enum LoaderError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Ontology not initialized")]
    NotInitialized,
}

#[derive(Debug, Serialize, Deserialize)]
struct ConceptNode {
    concept_id: String,
    name: String,
    level: i32,
    description: Option<String>,
    parent_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ConceptsResponse {
    concepts: Vec<ConceptNode>,
}

pub struct OntologyLoader {
    client: Arc<HelixClient>,
}

impl OntologyLoader {
    pub fn new(client: Arc<HelixClient>) -> Self {
        Self { client }
    }

    pub async fn check_initialized(&self) -> Result<bool, LoaderError> {
        let result: serde_json::Value = self.client
            .execute_query("checkOntologyInitialized", &serde_json::json!({}))
            .await
            .map_err(|e| LoaderError::Database(e.to_string()))?;

        Ok(result.get("thing").is_some())
    }

    pub async fn initialize_base(&self) -> Result<(), LoaderError> {
        let _: () = self.client
            .execute_query("initializeBaseOntology", &serde_json::json!({}))
            .await
            .map_err(|e| LoaderError::Database(e.to_string()))?;
        
        info!("Base ontology initialized");
        Ok(())
    }

    pub async fn load_base_ontology(&self) -> Result<(HashMap<String, Concept>, Vec<ConceptRelation>), LoaderError> {
        info!("Loading base ontology");

        if !self.check_initialized().await? {
            info!("Ontology not initialized - creating base ontology");
            self.initialize_base().await?;
        }

        let response: ConceptsResponse = self.client
            .execute_query("getAllConcepts", &serde_json::json!({}))
            .await
            .map_err(|e| LoaderError::Database(e.to_string()))?;

        let mut concepts = HashMap::new();
        let mut relations = Vec::new();

        for node in response.concepts {
            let concept = Concept {
                concept_id: node.concept_id.clone(),
                name: node.name,
                concept_type: if node.level <= 2 { ConceptType::Abstract } else { ConceptType::Concrete },
                description: node.description.unwrap_or_default(),
                parent_concept: node.parent_id.clone(),
                level: node.level as u8,
            };
            concepts.insert(node.concept_id.clone(), concept.clone());

            if let Some(parent_id) = node.parent_id {
                relations.push(ConceptRelation {
                    from_concept: parent_id,
                    to_concept: node.concept_id,
                    relation_type: RelationType::HasSubtype,
                });
            }
        }

        info!("Loaded {} concepts and {} relations", concepts.len(), relations.len());
        Ok((concepts, relations))
    }
}