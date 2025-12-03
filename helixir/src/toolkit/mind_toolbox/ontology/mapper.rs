

use std::collections::HashMap;

use lazy_static::lazy_static;


#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConceptType {
    
    Preference,
    
    Skill,
    
    Goal,
    
    Opinion,
    
    Fact,
    
    Action,
    
    Experience,
    
    Achievement,
}


#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextConcept {
    
    pub id: String,
    
    pub name: String,
    
    pub concept_type: ConceptType,
}


#[derive(Debug, Clone)]
pub struct ConceptMatch {
    
    pub concept: TextConcept,
    
    pub confidence: f64,
    
    pub matched_keywords: Vec<String>,
}

lazy_static! {
    
    static ref CONCEPT_KEYWORDS: HashMap<ConceptType, Vec<&'static str>> = {
        let mut m = HashMap::new();
        m.insert(ConceptType::Preference, vec![
            "like", "love", "prefer", "favorite", "enjoy", "hate", "dislike"
        ]);
        m.insert(ConceptType::Skill, vec![
            "can", "able to", "skilled at", "expert in", "know how", "proficient"
        ]);
        m.insert(ConceptType::Goal, vec![
            "want", "goal", "aim", "plan", "wish", "hope", "intend"
        ]);
        m.insert(ConceptType::Opinion, vec![
            "think", "believe", "feel", "opinion", "view", "consider"
        ]);
        m.insert(ConceptType::Fact, vec![
            "fact", "is", "has", "knows", "information", "data"
        ]);
        m.insert(ConceptType::Action, vec![
            "did", "does", "doing", "performed", "executed", "ran"
        ]);
        m.insert(ConceptType::Experience, vec![
            "experienced", "went through", "encounter", "witnessed"
        ]);
        m.insert(ConceptType::Achievement, vec![
            "completed", "finished", "achieved", "success", "accomplished"
        ]);
        m
    };
}


pub struct ConceptMapper;

impl ConceptMapper {
    
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    
    #[must_use]
    pub fn map_to_concepts(&self, text: &str, top_k: usize) -> Vec<ConceptMatch> {
        let text_lower = text.to_lowercase();
        let mut matches: Vec<ConceptMatch> = Vec::new();

        for (concept_type, keywords) in CONCEPT_KEYWORDS.iter() {
            let matched: Vec<String> = keywords
                .iter()
                .filter(|kw| text_lower.contains(*kw))
                .map(|s| (*s).to_string())
                .collect();

            if !matched.is_empty() {
                let confidence = matched.len() as f64 / keywords.len() as f64;
                let concept_name = format!("{:?}", concept_type);

                matches.push(ConceptMatch {
                    concept: TextConcept {
                        id: concept_name.clone(),
                        name: concept_name,
                        concept_type: concept_type.clone(),
                    },
                    confidence,
                    matched_keywords: matched,
                });
            }
        }

        
        matches.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

        
        matches.into_iter().take(top_k).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_preference() {
        let mapper = ConceptMapper::new(HashMap::new());
        let matches = mapper.map_to_concepts("I love programming", 3);

        assert!(!matches.is_empty());
        assert_eq!(matches[0].concept.concept_type, ConceptType::Preference);
    }

    #[test]
    fn test_map_skill() {
        let mapper = ConceptMapper::new(HashMap::new());
        let matches = mapper.map_to_concepts("I can write Rust code", 3);

        assert!(!matches.is_empty());
        let has_skill = matches
            .iter()
            .any(|m| m.concept.concept_type == ConceptType::Skill);
        assert!(has_skill);
    }

    #[test]
    fn test_no_match() {
        let mapper = ConceptMapper::new(HashMap::new());
        let matches = mapper.map_to_concepts("xyz123", 3);

        assert!(matches.is_empty());
    }

    #[test]
    fn test_case_insensitive() {
        let mapper = ConceptMapper::new(HashMap::new());
        let matches1 = mapper.map_to_concepts("I LOVE RUST", 3);
        let matches2 = mapper.map_to_concepts("i love rust", 3);

        assert_eq!(matches1.len(), matches2.len());
    }
}
