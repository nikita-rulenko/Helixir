

use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntoSearchConfig {
    pub concept_weight: f64,
    pub tag_weight: f64,
    pub vector_weight: f64,
    pub graph_weight: f64,
    pub temporal_weight: f64,
    pub max_concept_depth: usize,
    pub include_related_concepts: bool,
    pub temporal_hours: Option<f64>,
    pub temporal_decay_rate: f64,
    pub min_concept_score: f64,
    pub min_final_score: f64,
    pub boost_exact_concept_match: f64,
    pub boost_tag_match: f64,
    pub max_concepts_per_query: usize,
    pub max_tags_per_query: usize,
    pub vector_top_k: usize,
    pub graph_depth: usize,
}

impl Default for OntoSearchConfig {
    fn default() -> Self {
        Self {
            concept_weight: 0.3,
            tag_weight: 0.15,
            vector_weight: 0.35,
            graph_weight: 0.1,
            temporal_weight: 0.1,
            max_concept_depth: 3,
            include_related_concepts: true,
            temporal_hours: None,
            temporal_decay_rate: 30.0,
            min_concept_score: 0.1,
            min_final_score: 0.2,
            boost_exact_concept_match: 0.2,
            boost_tag_match: 0.1,
            max_concepts_per_query: 5,
            max_tags_per_query: 10,
            vector_top_k: 20,
            graph_depth: 2,
        }
    }
}

impl OntoSearchConfig {
    
    pub fn from_mode(mode: &str) -> Self {
        match mode {
            "recent" => Self {
                temporal_weight: 0.4,
                vector_weight: 0.3,
                concept_weight: 0.2,
                tag_weight: 0.05,
                graph_weight: 0.05,
                temporal_hours: Some(24.0),
                temporal_decay_rate: 7.0,
                min_final_score: 0.15,
                ..Default::default()
            },
            "contextual" => Self {
                concept_weight: 0.4,
                vector_weight: 0.3,
                tag_weight: 0.15,
                temporal_weight: 0.1,
                graph_weight: 0.05,
                include_related_concepts: true,
                boost_exact_concept_match: 0.3,
                ..Default::default()
            },
            "deep" => Self {
                graph_weight: 0.3,
                concept_weight: 0.25,
                vector_weight: 0.25,
                temporal_weight: 0.1,
                tag_weight: 0.1,
                graph_depth: 3,
                max_concept_depth: 4,
                ..Default::default()
            },
            "full" => Self {
                concept_weight: 0.25,
                vector_weight: 0.25,
                graph_weight: 0.2,
                tag_weight: 0.15,
                temporal_weight: 0.15,
                graph_depth: 2,
                include_related_concepts: true,
                ..Default::default()
            },
            _ => Self::default(),
        }
    }
}

