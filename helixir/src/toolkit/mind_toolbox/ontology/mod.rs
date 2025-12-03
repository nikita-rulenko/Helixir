

pub mod mapper;
pub mod models;
pub mod loader;
pub mod hierarchy;
pub mod classifier;

pub use mapper::{TextConcept, ConceptMapper, ConceptMatch};
pub use models::Concept;
pub use models::{ConceptType, ConceptRelation, RelationType, OntologyStats};
pub use loader::{OntologyLoader, LoaderError};
pub use hierarchy::{HierarchyTraverser, HierarchyError};
pub use classifier::ConceptClassifier;

use crate::db::HelixClient;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
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
        if self.concepts_cache.read().unwrap().contains_key(&concept.concept_id) {
            return Err(OntologyError::AlreadyExists(concept.concept_id));
        }
        self.concepts_cache.write().unwrap().insert(concept.concept_id.clone(), concept);
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

    pub fn map_memory_to_concepts(&self, content: &str, memory_type: Option<&str>) -> Vec<ConceptMatch> {
        if !self.is_loaded {
            return Vec::new();
        }
        self.mapper.map_to_concepts(content, 30)
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