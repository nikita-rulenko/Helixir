

use super::super::config::OntoSearchConfig;
use super::super::models::ConceptMatch;


const CONCEPT_KEYWORDS: &[(&str, &str)] = &[
    ("preference", "Preference"), ("like", "Preference"), ("love", "Preference"),
    ("enjoy", "Preference"), ("hate", "Preference"), ("dislike", "Preference"),
    ("skill", "Skill"), ("know", "Skill"), ("learn", "Skill"),
    ("expert", "Skill"), ("master", "Skill"),
    ("goal", "Goal"), ("want", "Goal"), ("plan", "Goal"),
    ("aim", "Goal"), ("objective", "Goal"),
    ("fact", "Fact"), ("remember", "Fact"), ("true", "Fact"), ("false", "Fact"),
    ("opinion", "Opinion"), ("think", "Opinion"), ("believe", "Opinion"), ("feel", "Opinion"),
    ("experience", "Experience"), ("did", "Experience"), ("happened", "Experience"),
    ("achievement", "Achievement"), ("completed", "Achievement"),
    ("finished", "Achievement"), ("succeeded", "Achievement"),
];


const KNOWN_TAGS: &[&str] = &[
    "python", "fastapi", "rust", "javascript", "typescript", "react",
    "django", "flask", "nodejs", "docker", "kubernetes", "aws", "gcp",
    "postgresql", "mongodb", "redis", "helixdb", "ollama", "openai",
    "async", "api", "backend", "frontend", "database", "graph",
    "work", "personal", "project", "home", "travel", "health",
    "finance", "learning", "career", "family",
    "ai", "ml", "memory", "llm", "embedding", "vector", "search",
    "programming", "coding", "development", "architecture",
];


pub fn classify_query_concepts(query: &str, config: &OntoSearchConfig) -> Vec<ConceptMatch> {
    let query_lower = query.to_lowercase();
    let mut concepts = Vec::new();

    for (keyword, concept_id) in CONCEPT_KEYWORDS {
        if query_lower.contains(keyword) {
            concepts.push(ConceptMatch {
                concept_id: (*concept_id).to_string(),
                confidence: 0.8,
                match_type: "exact".to_string(),
            });
        }
    }

    concepts.truncate(config.max_concepts_per_query);
    concepts
}


pub fn extract_query_tags(query: &str, config: &OntoSearchConfig) -> Vec<String> {
    let query_lower = query.to_lowercase();
    let mut tags = Vec::new();

    for tag in KNOWN_TAGS {
        if query_lower.contains(tag) {
            tags.push((*tag).to_string());
        }
    }

    tags.truncate(config.max_tags_per_query);
    tags
}

