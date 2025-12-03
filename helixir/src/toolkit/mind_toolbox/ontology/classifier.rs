use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use lazy_static::lazy_static;
use super::models::Concept;

lazy_static! {
    static ref KEYWORD_PATTERNS: HashMap<String, Vec<String>> = {
        let mut map = HashMap::new();
        map.insert("Preference".to_string(), vec![
            "love".to_string(), "like".to_string(), "prefer".to_string(),
            "enjoy".to_string(), "favorite".to_string(), "hate".to_string(), "dislike".to_string()
        ]);
        map.insert("Skill".to_string(), vec![
            "can".to_string(), "know how".to_string(), "able to".to_string(),
            "expert".to_string(), "proficient".to_string(), "skilled".to_string()
        ]);
        map.insert("Fact".to_string(), vec![
            "is".to_string(), "are".to_string(), "was".to_string(),
            "were".to_string(), "has".to_string(), "have".to_string()
        ]);
        map.insert("Goal".to_string(), vec![
            "want".to_string(), "plan".to_string(), "goal".to_string(),
            "aim".to_string(), "intend".to_string(), "wish".to_string()
        ]);
        map.insert("Opinion".to_string(), vec![
            "think".to_string(), "believe".to_string(), "feel".to_string(),
            "opinion".to_string(), "view".to_string()
        ]);
        map.insert("Experience".to_string(), vec![
            "did".to_string(), "went".to_string(), "saw".to_string(),
            "experienced".to_string(), "happened".to_string()
        ]);
        map.insert("Achievement".to_string(), vec![
            "completed".to_string(), "finished".to_string(), "achieved".to_string(),
            "accomplished".to_string(), "built".to_string()
        ]);
        map
    };
}

pub struct ConceptClassifier {
    concepts: Arc<RwLock<HashMap<String, Concept>>>,
    keyword_patterns: HashMap<String, Vec<String>>,
}

impl ConceptClassifier {
    pub fn new(concepts: Arc<RwLock<HashMap<String, Concept>>>) -> Self {
        Self {
            concepts,
            keyword_patterns: KEYWORD_PATTERNS.clone(),
        }
    }

    pub fn classify(&self, text: &str, min_confidence: f64) -> Vec<(String, f64)> {
        let mut scores = Vec::new();
        let text_lower = text.to_lowercase();

        for (concept_id, keywords) in &self.keyword_patterns {
            let mut matched = 0;
            for keyword in keywords {
                if text_lower.contains(keyword) {
                    matched += 1;
                }
            }

            if matched > 0 {
                let score = matched as f64 / keywords.len() as f64;
                if score >= min_confidence {
                    scores.push((concept_id.clone(), score));
                }
            }
        }

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scores
    }

    pub fn suggest_concepts(&self, text: &str, top_n: usize) -> Vec<String> {
        let results = self.classify(text, 0.1);
        results.into_iter().take(top_n).map(|(id, _)| id).collect()
    }
}