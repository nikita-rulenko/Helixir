pub mod classifier;
pub mod hierarchy;
pub mod loader;
pub mod mapper;
pub mod models;

pub use classifier::ConceptClassifier;
pub use hierarchy::{HierarchyError, HierarchyTraverser};
pub use loader::{LoaderError, OntologyLoader};
pub use mapper::{ConceptMapper, ConceptMatch, TextConcept};
pub use models::Concept;
pub use models::{ConceptRelation, ConceptType, OntologyStats, RelationType};

use crate::db::HelixClient;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use thiserror::Error;
use tracing::info;

#[derive(Error, Debug)]
pub enum OntologyError {
    #[error("Loader error: {0}")]
    Loader(#[from] LoaderError),
    #[error("Hierarchy error: {0}")]
    Hierarchy(#[from] HierarchyError),
    #[error("Ontology not loaded")]
    NotLoaded,
    #[error("Concept already exists: {0}")]
    AlreadyExists(String),
}

pub struct OntologyManager {
    client: Arc<HelixClient>,
    loader: OntologyLoader,
    hierarchy: HierarchyTraverser,
    classifier: ConceptClassifier,
    mapper: ConceptMapper,
    concepts_cache: Arc<RwLock<HashMap<String, Concept>>>,
    relations_cache: Vec<ConceptRelation>,
    is_loaded: bool,
}

impl OntologyManager {
    pub fn new(client: Arc<HelixClient>) -> Self {
        let concepts_cache = Arc::new(RwLock::new(HashMap::new()));
        Self {
            loader: OntologyLoader::new(client.clone()),
            hierarchy: HierarchyTraverser::new(concepts_cache.clone()),
            classifier: ConceptClassifier::new(concepts_cache.clone()),
            mapper: ConceptMapper::new(),
            client,
            concepts_cache,
            relations_cache: Vec::new(),
            is_loaded: false,
        }
    }

    pub async fn load(&mut self) -> Result<(), OntologyError> {
        info!("Loading ontology");
        let (concepts, relations) = self.loader.load_base_ontology().await?;
        *self.concepts_cache.write().unwrap() = concepts;
        self.relations_cache = relations;
        self.is_loaded = true;
        Ok(())
    }

    pub fn get_concept(&self, id: &str) -> Option<Concept> {
        self.concepts_cache.read().unwrap().get(id).cloned()
    }

    pub fn add_concept(&mut self, concept: Concept) -> Result<(), OntologyError> {
        if self
            .concepts_cache
            .read()
            .unwrap()
            .contains_key(&concept.concept_id)
        {
            return Err(OntologyError::AlreadyExists(concept.concept_id));
        }
        self.concepts_cache
            .write()
            .unwrap()
            .insert(concept.concept_id.clone(), concept);
        Ok(())
    }

    pub fn get_subtypes(&self, id: &str) -> Result<Vec<Concept>, OntologyError> {
        if !self.is_loaded {
            return Err(OntologyError::NotLoaded);
        }
        Ok(self.hierarchy.get_subtypes(id)?)
    }

    pub fn get_ancestors(&self, id: &str) -> Vec<Concept> {
        if !self.is_loaded {
            return Vec::new();
        }
        self.hierarchy.get_ancestors(id)
    }

    pub fn classify_text(&self, text: &str, min_confidence: f64) -> Vec<(String, f64)> {
        if !self.is_loaded {
            return Vec::new();
        }
        self.classifier.classify(text, min_confidence)
    }

    pub fn map_memory_to_concepts(
        &self,
        content: &str,
        memory_type: Option<&str>,
    ) -> Vec<ConceptMatch> {
        let mut matches = if self.is_loaded {
            self.mapper.map_to_concepts(content, 30)
        } else {
            Vec::new()
        };

        if let Some(mt) = memory_type {
            let concept_pair = match mt.to_lowercase().as_str() {
                "preference" => Some(("Preference", mapper::ConceptType::Preference)),
                "skill" => Some(("Skill", mapper::ConceptType::Skill)),
                "goal" => Some(("Goal", mapper::ConceptType::Goal)),
                "opinion" => Some(("Opinion", mapper::ConceptType::Opinion)),
                "fact" => Some(("Fact", mapper::ConceptType::Fact)),
                "action" => Some(("Action", mapper::ConceptType::Action)),
                "experience" => Some(("Experience", mapper::ConceptType::Experience)),
                "achievement" => Some(("Achievement", mapper::ConceptType::Achievement)),
                _ => None,
            };

            if let Some((cid, ctype)) = concept_pair {
                let already_linked = matches.iter().any(|m| m.concept.id == cid);
                if !already_linked {
                    matches.push(ConceptMatch {
                        concept: TextConcept {
                            id: cid.to_string(),
                            name: cid.to_string(),
                            concept_type: ctype,
                        },
                        confidence: 0.9,
                        matched_keywords: vec![format!("memory_type={}", mt)],
                    });
                }
            }
        }

        matches
    }

    pub fn get_stats(&self) -> OntologyStats {
        let concepts = self.concepts_cache.read().unwrap();
        OntologyStats {
            total_concepts: concepts.len(),
            total_relations: self.relations_cache.len(),
            concepts_by_type: HashMap::new(),
            max_depth: 0,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.is_loaded
    }
}
