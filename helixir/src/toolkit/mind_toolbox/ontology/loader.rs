use super::models::{Concept, ConceptRelation, ConceptType, RelationType};
use crate::db::HelixClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tracing::info;

#[derive(Debug, Error)]
pub enum LoaderError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Ontology not initialized")]
    NotInitialized,
}

#[derive(Debug, Serialize, Deserialize)]
struct ConceptNode {
    /// HelixDB internal node UUID (time-ordered) — used by the duplicate
    /// self-heal to keep the earliest copy per concept_id.
    #[serde(default)]
    id: String,
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
        // `checkOntologyInitialized` does `N<Concept>::...::FIRST`, which raises
        // GRAPH_ERROR "No value found" on an EMPTY ontology (the #19 empty-
        // traversal pattern) rather than returning an empty result. Treat that
        // as "not initialized" so the loader self-heals by re-seeding the base
        // ontology — otherwise a wiped/empty ontology makes every fresh MCP die
        // at startup instead of rebuilding itself.
        match self
            .client
            .execute_query::<serde_json::Value, _>(
                "checkOntologyInitialized",
                &serde_json::json!({}),
            )
            .await
        {
            Ok(result) => Ok(result.get("thing").is_some()),
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                if msg.contains("no value found") || msg.contains("graph_error") {
                    Ok(false)
                } else {
                    Err(LoaderError::Database(e.to_string()))
                }
            }
        }
    }

    pub async fn initialize_base(&self) -> Result<(), LoaderError> {
        // `initializeBaseOntology` RETURNS the created root ({"thing": {...}}),
        // so it must be decoded as a Value — deserializing into `()` fails with
        // "error decoding response body" and aborts the self-heal. (Latent until
        // the re-seed path actually ran, i.e. an emptied ontology.)
        let _: serde_json::Value = self
            .client
            .execute_query("initializeBaseOntology", &serde_json::json!({}))
            .await
            .map_err(|e| LoaderError::Database(e.to_string()))?;

        info!("Base ontology initialized");
        Ok(())
    }

    pub async fn load_base_ontology(
        &self,
    ) -> Result<(HashMap<String, Concept>, Vec<ConceptRelation>), LoaderError> {
        info!("Loading base ontology");

        if !self.check_initialized().await? {
            info!("Ontology not initialized - creating base ontology");
            self.initialize_base().await?;
        }

        let mut response: ConceptsResponse = self
            .client
            .execute_query("getAllConcepts", &serde_json::json!({}))
            .await
            .map_err(|e| LoaderError::Database(e.to_string()))?;

        // Self-heal duplicated trees (#67): retry-amplified seeding once left
        // FOUR copies of the base tree. Live lookups (`WHERE … ::FIRST`)
        // resolve in insertion order, so the earliest copy is the one all
        // INSTANCE_OF edges actually target — keep the earliest node per
        // concept_id (ids are time-ordered) and drop the later phantoms.
        {
            let mut earliest: HashMap<&str, &str> = HashMap::new();
            for n in &response.concepts {
                let e = earliest.entry(n.concept_id.as_str()).or_insert(&n.id);
                if n.id.as_str() < *e {
                    *e = &n.id;
                }
            }
            let phantoms: Vec<String> = response
                .concepts
                .iter()
                .filter(|n| earliest.get(n.concept_id.as_str()) != Some(&n.id.as_str()))
                .map(|n| n.id.clone())
                .collect();
            if !phantoms.is_empty() {
                tracing::warn!(
                    "Ontology self-heal: dropping {} duplicate concept node(s) \
                     (multiple seeded copies detected)",
                    phantoms.len()
                );
                for id in &phantoms {
                    if let Err(e) = self
                        .client
                        .execute_query::<serde_json::Value, _>(
                            "dropConceptByInternalId",
                            &serde_json::json!({ "concept_internal_id": id }),
                        )
                        .await
                    {
                        // Pre-#67 deployments don't have the query yet — the
                        // in-memory map below still dedupes, so only warn.
                        tracing::warn!("Ontology self-heal: drop {} failed: {}", id, e);
                        break;
                    }
                }
                let keep: std::collections::HashSet<String> =
                    earliest.values().map(|s| s.to_string()).collect();
                response.concepts.retain(|n| keep.contains(&n.id));
            }
        }

        let mut concepts = HashMap::new();
        let mut relations = Vec::new();

        for node in response.concepts {
            let concept = Concept {
                concept_id: node.concept_id.clone(),
                name: node.name,
                concept_type: if node.level <= 2 {
                    ConceptType::Abstract
                } else {
                    ConceptType::Concrete
                },
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

        info!(
            "Loaded {} concepts and {} relations",
            concepts.len(),
            relations.len()
        );
        Ok((concepts, relations))
    }
}
