use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use super::models::Concept;
use tracing::{debug, warn};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum HierarchyError {
    #[error("Concept not found: {0}")]
    NotFound(String),
}

pub struct HierarchyTraverser {
    concepts_cache: Arc<RwLock<HashMap<String, Concept>>>,
}

impl HierarchyTraverser {
    pub fn new(cache: Arc<RwLock<HashMap<String, Concept>>>) -> Self {
        Self { concepts_cache: cache }
    }

    pub fn get_subtypes(&self, concept_id: &str) -> Result<Vec<Concept>, HierarchyError> {
        debug!("Getting subtypes for concept: {}", concept_id);
        
        let cache = self.concepts_cache.read().unwrap();
        let mut subtypes = Vec::new();
        
        for concept in cache.values() {
            if let Some(parent_id) = &concept.parent_concept {
                if parent_id == concept_id {
                    subtypes.push(concept.clone());
                }
            }
        }
        
        Ok(subtypes)
    }

    pub fn get_ancestors(&self, concept_id: &str) -> Vec<Concept> {
        let cache = self.concepts_cache.read().unwrap();
        let mut ancestors = Vec::new();
        let mut current_id = concept_id;

        while let Some(concept) = cache.get(current_id) {
            if let Some(parent_id) = &concept.parent_concept {
                if let Some(parent) = cache.get(parent_id) {
                    ancestors.push(parent.clone());
                    current_id = parent_id;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        ancestors
    }

    pub fn get_depth(&self, concept_id: &str) -> usize {
        self.get_ancestors(concept_id).len()
    }
}