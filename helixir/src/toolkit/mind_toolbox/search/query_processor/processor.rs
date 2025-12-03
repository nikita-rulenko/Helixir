use std::sync::Arc;
use tracing::{debug, info, warn};
use crate::llm::providers::base::LlmProvider;
use super::models::ProcessedQuery;
use super::patterns::{detect_intent, intent_to_concept, EXPANSION_MAPPINGS};


pub struct QueryProcessor {
    llm_provider: Option<Arc<dyn LlmProvider>>,
    enable_expansion: bool,
    max_expansions: usize,
}

impl QueryProcessor {
    pub fn new(
        llm_provider: Option<Arc<dyn LlmProvider>>,
        enable_expansion: bool,
        max_expansions: usize,
    ) -> Self {
        Self {
            llm_provider,
            enable_expansion,
            max_expansions,
        }
    }
    
    
    pub fn process(&self, query: &str) -> ProcessedQuery {
        debug!("Processing query: {}", query);
        
        if query.trim().is_empty() {
            return ProcessedQuery::empty(query);
        }
        
        
        let detected_intents_raw = detect_intent(query);
        let detected_intents: Vec<String> = detected_intents_raw.iter().map(|&s| s.to_string()).collect();
        
        
        let concept_hints = self.intents_to_concepts(&detected_intents);
        
        
        let expanded_terms = if self.enable_expansion {
            self.expand_query(query)
        } else {
            Vec::new()
        };
        
        
        let enhanced_query = self.build_enhanced_query(query, &expanded_terms);
        
        
        let suggested_mode = self.suggest_mode(&detected_intents, query);
        
        
        let confidence = self.calculate_confidence(&detected_intents, &expanded_terms);
        
        info!("Query processed with {} intents, confidence: {}", detected_intents.len(), confidence);
        
        ProcessedQuery {
            original_query: query.to_string(),
            enhanced_query,
            detected_intents,
            concept_hints,
            expanded_terms,
            suggested_mode,
            confidence,
        }
    }
    
    
    pub async fn process_with_llm(&self, query: &str) -> ProcessedQuery {
        debug!("Processing query with LLM: {}", query);
        
        
        let mut result = self.process(query);
        
        
        if let Some(llm) = &self.llm_provider {
            let system_prompt = "You are a query analyzer. Respond only with valid JSON.";
            let user_prompt = format!(
                r#"Analyze the following user query and provide insights in JSON format:
Query: "{}"

Return a JSON object with:
- intents: array of detected intents (preference, skill, goal, fact, opinion, experience, recent)
- concepts: array of relevant ontology concepts
- expansions: array of terms to expand the query
- mode: suggested search mode (recent, contextual, deep, or null)"#,
                query
            );
            
            match llm.generate(system_prompt, &user_prompt, Some("json_object")).await {
                Ok((response, _metadata)) => {
                    debug!("LLM response received: {}", response);
                    
                    
                    if let Ok(llm_insights) = serde_json::from_str::<serde_json::Value>(&response) {
                        
                        if let Some(intents) = llm_insights.get("intents").and_then(|v| v.as_array()) {
                            for intent in intents {
                                if let Some(intent_str) = intent.as_str() {
                                    if !result.detected_intents.contains(&intent_str.to_string()) {
                                        result.detected_intents.push(intent_str.to_string());
                                    }
                                }
                            }
                        }
                        
                        if let Some(concepts) = llm_insights.get("concepts").and_then(|v| v.as_array()) {
                            for concept in concepts {
                                if let Some(concept_str) = concept.as_str() {
                                    if !result.concept_hints.contains(&concept_str.to_string()) {
                                        result.concept_hints.push(concept_str.to_string());
                                    }
                                }
                            }
                        }
                        
                        if let Some(expansions) = llm_insights.get("expansions").and_then(|v| v.as_array()) {
                            for expansion in expansions {
                                if let Some(expansion_str) = expansion.as_str() {
                                    if !result.expanded_terms.contains(&expansion_str.to_string()) {
                                        result.expanded_terms.push(expansion_str.to_string());
                                    }
                                }
                            }
                        }
                        
                        if let Some(mode) = llm_insights.get("mode").and_then(|v| v.as_str()) {
                            result.suggested_mode = Some(mode.to_string());
                        }
                        
                        
                        result.confidence = (result.confidence + 0.2).min(1.0);
                        
                        info!("Query enhanced with LLM insights");
                    } else {
                        warn!("Failed to parse LLM response as JSON");
                    }
                }
                Err(e) => {
                    warn!("LLM processing failed: {}", e);
                }
            }
        } else {
            debug!("No LLM provider available, using rule-based only");
        }
        
        result
    }
    
    
    fn intents_to_concepts(&self, intents: &[String]) -> Vec<String> {
        let mut concepts = Vec::new();
        for intent in intents {
            if let Some(concept) = intent_to_concept(intent) {
                concepts.push(concept.to_string());
            }
        }
        concepts
    }
    
    fn expand_query(&self, query: &str) -> Vec<String> {
        let mut expansions = Vec::new();
        let query_lower = query.to_lowercase();
        
        for (term, synonyms) in EXPANSION_MAPPINGS.iter() {
            if query_lower.contains(term) {
                for &synonym in synonyms {
                    if expansions.len() < self.max_expansions {
                        expansions.push(synonym.to_string());
                    }
                }
            }
        }
        
        expansions
    }
    
    fn build_enhanced_query(&self, query: &str, expansions: &[String]) -> String {
        if expansions.is_empty() {
            return query.to_string();
        }
        
        let mut enhanced = query.to_string();
        for expansion in expansions.iter().take(self.max_expansions) {
            enhanced.push(' ');
            enhanced.push_str(expansion);
        }
        
        enhanced
    }
    
    fn suggest_mode(&self, intents: &[String], query: &str) -> Option<String> {
        let query_lower = query.to_lowercase();
        
        
        if intents.contains(&"recent".to_string()) || 
           query_lower.contains("today") || 
           query_lower.contains("yesterday") || 
           query_lower.contains("recently") ||
           query_lower.contains("lately") ||
           query_lower.contains("just now") ||
           query_lower.contains("this week") {
            return Some("recent".to_string());
        }
        
        
        if query_lower.contains("all") || 
           query_lower.contains("everything") || 
           query_lower.contains("complete") ||
           query_lower.contains("full") ||
           query_lower.contains("entire") {
            return Some("deep".to_string());
        }
        
        
        if !intents.is_empty() {
            return Some("contextual".to_string());
        }
        
        None
    }
    
    fn calculate_confidence(&self, intents: &[String], expansions: &[String]) -> f64 {
        let mut confidence = 0.3; 
        
        
        let intent_bonus = (intents.len() as f64 * 0.15).min(0.3);
        confidence += intent_bonus;
        
        
        let expansion_bonus = (expansions.len() as f64 * 0.05).min(0.2);
        confidence += expansion_bonus;
        
        confidence.min(1.0)
    }
}