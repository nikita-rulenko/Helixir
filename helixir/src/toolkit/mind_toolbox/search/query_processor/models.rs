use serde::{Deserialize, Serialize};
use std::collections::HashMap;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessedQuery {
    
    pub original_query: String,
    
    pub enhanced_query: String,
    
    pub detected_intents: Vec<String>,
    
    pub concept_hints: Vec<String>,
    
    pub expanded_terms: Vec<String>,
    
    pub suggested_mode: Option<String>,
    
    pub confidence: f64,
}

impl ProcessedQuery {
    
    pub fn empty(query: &str) -> Self {
        Self {
            original_query: query.to_string(),
            enhanced_query: query.to_string(),
            detected_intents: Vec::new(),
            concept_hints: Vec::new(),
            expanded_terms: Vec::new(),
            suggested_mode: None,
            confidence: 0.0,
        }
    }
    
    
    pub fn to_dict(&self) -> HashMap<String, serde_json::Value> {
        let mut map = HashMap::new();
        map.insert("original_query".to_string(), serde_json::Value::String(self.original_query.clone()));
        map.insert("enhanced_query".to_string(), serde_json::Value::String(self.enhanced_query.clone()));
        map.insert("detected_intents".to_string(), serde_json::Value::Array(
            self.detected_intents.iter().map(|s| serde_json::Value::String(s.clone())).collect()
        ));
        map.insert("concept_hints".to_string(), serde_json::Value::Array(
            self.concept_hints.iter().map(|s| serde_json::Value::String(s.clone())).collect()
        ));
        map.insert("expanded_terms".to_string(), serde_json::Value::Array(
            self.expanded_terms.iter().map(|s| serde_json::Value::String(s.clone())).collect()
        ));
        map.insert("suggested_mode".to_string(), match &self.suggested_mode {
            Some(mode) => serde_json::Value::String(mode.clone()),
            None => serde_json::Value::Null,
        });
        map.insert("confidence".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(self.confidence).unwrap_or(serde_json::Number::from(0))));
        map
    }
}

impl Default for ProcessedQuery {
    fn default() -> Self {
        Self {
            original_query: String::new(),
            enhanced_query: String::new(),
            detected_intents: Vec::new(),
            concept_hints: Vec::new(),
            expanded_terms: Vec::new(),
            suggested_mode: None,
            confidence: 0.0,
        }
    }
}