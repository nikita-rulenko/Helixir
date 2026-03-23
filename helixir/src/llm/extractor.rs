

use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use super::providers::base::{LlmProvider, LlmProviderError};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    
    pub memories: Vec<ExtractedMemory>,
    
    pub entities: Vec<ExtractedEntity>,
    
    pub relations: Vec<ExtractedRelation>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedMemory {
    
    pub text: String,
    
    pub memory_type: String,
    
    pub certainty: i32,
    
    pub importance: i32,
    
    pub entities: Vec<String>,

    #[serde(default)]
    pub context: Option<String>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntityRelation {
    pub target_entity: String,
    pub relationship_type: String,
    #[serde(default = "default_strength_i64")]
    pub strength: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    
    pub id: String,
    
    pub name: String,
    
    #[serde(rename = "type")]
    pub entity_type: String,

    #[serde(default)]
    pub relations: Option<Vec<ExtractedEntityRelation>>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRelation {
    #[serde(default)]
    pub from_memory_index: Option<usize>,
    #[serde(default)]
    pub to_memory_index: Option<usize>,
    #[serde(default)]
    pub from_memory_content: String,
    #[serde(default)]
    pub to_memory_content: String,
    
    pub relation_type: String,
    
    #[serde(default = "default_strength")]
    pub strength: i32,
    
    #[serde(default = "default_confidence")]
    pub confidence: i32,
    
    #[serde(default)]
    pub explanation: String,
}

fn default_strength() -> i32 { 80 }
fn default_strength_i64() -> i64 { 80 }
fn default_confidence() -> i32 { 80 }


pub struct LlmExtractor<P: LlmProvider> {
    provider: P,
}

impl<P: LlmProvider> LlmExtractor<P> {
    
    #[must_use]
    pub fn new(provider: P) -> Self {
        Self { provider }
    }

    
    pub async fn extract(
        &self,
        text: &str,
        user_id: &str,
        extract_entities: bool,
        extract_relations: bool,
    ) -> Result<ExtractionResult, LlmProviderError> {
        let preview: String = text.chars().take(50).collect();
        info!(
            "Extracting memories from text: {}... (user={})",
            preview,
            user_id
        );

        let system_prompt = self.build_system_prompt(extract_entities, extract_relations);
        let user_prompt = format!("Extract information from this text:\n\n{}", text);

        let (response, _metadata) = self
            .provider
            .generate(&system_prompt, &user_prompt, Some("json_object"))
            .await?;

        
        match serde_json::from_str::<ExtractionResult>(&response) {
            Ok(result) => {
                debug!(
                    "Extracted {} memories, {} entities, {} relations",
                    result.memories.len(),
                    result.entities.len(),
                    result.relations.len()
                );
                Ok(result)
            }
            Err(e) => {
                warn!("Failed to parse extraction result: {}", e);
                
                Ok(ExtractionResult {
                    memories: Vec::new(),
                    entities: Vec::new(),
                    relations: Vec::new(),
                })
            }
        }
    }

    
    fn build_system_prompt(&self, extract_entities: bool, extract_relations: bool) -> String {
        let mut prompt = String::from(
            r#"You are a memory extraction system. Analyze the text and extract structured information.

Each extracted memory MUST have a memory_type from EXACTLY one of these 7 types:

- "fact": Objective information, knowledge, or statements about the world.
  Example: "The Earth orbits the Sun" or "Rust is a systems programming language"
- "preference": Personal likes, dislikes, tastes, or favorites.
  Example: "I love playing chess" or "I prefer dark mode"
- "skill": An ability, competency, or expertise the person possesses.
  Example: "I am skilled at pattern recognition" or "I can write Rust code"
  NOTE: "I am skilled at X" or "I can do X" is ALWAYS a skill, never a fact.
- "goal": Something the person wants to achieve, a plan, or an aspiration.
  Example: "My goal is to become a grandmaster" or "I want to build a database engine"
- "opinion": A subjective belief, judgment, or viewpoint.
  Example: "In my opinion, Rust is the best language" or "I think chess is the ultimate test"
- "experience": A past event, situation, or something that happened to the person.
  Example: "I lived in Tokyo for two years" or "I went through a career change"
- "achievement": A specific accomplishment, milestone, or completed goal.
  Example: "I achieved winning a tournament" or "I built a working compiler"
  NOTE: "I achieved X" or "I built/completed/finished X" is ALWAYS an achievement, never an experience.

Output JSON with this structure:
{
  "memories": [
    {
      "text": "atomic standalone fact",
      "memory_type": "fact|preference|skill|goal|opinion|experience|achievement",
      "certainty": 80,
      "importance": 50,
      "entities": ["entity_id1", "entity_id2"],
      "context": "work|personal|health|project:name|conversation:topic"
    }
  ]

For each memory, optionally include "context" — the situational context this memory applies in:
- Examples: "work", "personal", "health", "project:name", "conversation:topic"
- Only set if the context is clearly identifiable from the text
- Omit or set to null if the context is ambiguous or universal"#,
        );

        if extract_entities {
            prompt.push_str(
                r#",
  "entities": [
    {
      "id": "unique_id",
      "name": "Entity Name",
      "type": "person|organization|location|concept|system",
      "relations": [
        {
          "target_entity": "other_entity_id",
          "relationship_type": "works_at|part_of|collaborates_with|uses|related_to",
          "strength": 80
        }
      ]
    }
  ]

For each entity, optionally include "relations" — connections to OTHER entities mentioned in the same text:
- target_entity: the "id" of the related entity (must reference another entity in this extraction)
- relationship_type: type of relationship (works_at, part_of, collaborates_with, uses, created_by, belongs_to, located_in, related_to, etc.)
- strength: 1-100 confidence in the relationship
- Omit "relations" if no inter-entity relationships are evident"#,
            );
        } else {
            prompt.push_str(r#",
  "entities": []"#);
        }

        if extract_relations {
            prompt.push_str(
                r#",
  "relations": [
    {
      "from_memory_index": 0,
      "to_memory_index": 1,
      "relation_type": "IMPLIES|BECAUSE|CONTRADICTS|SUPPORTS",
      "strength": 80,
      "confidence": 80,
      "explanation": "Why this relation exists"
    }
  ]"#,
            );
            prompt.push_str(r#"

Relations use INDICES into the memories array (0-based).
Relation types:
- IMPLIES: memory A logically leads to or suggests memory B
- BECAUSE: memory A is the reason/cause for memory B
- CONTRADICTS: memory A conflicts with memory B
- SUPPORTS: memory A provides evidence for memory B

Always look for causal and logical connections between extracted memories. If the text contains cause-effect, reasoning, or contradictions, you MUST extract them as relations.

CRITICAL relation extraction rules:
- If memory A is the REASON for memory B → relation_type: "BECAUSE"
- If memory A logically LEADS TO memory B → relation_type: "IMPLIES"
- If memory A CONFLICTS with memory B → relation_type: "CONTRADICTS"
- If memory A provides EVIDENCE for memory B → relation_type: "SUPPORTS"
- Even for 2 memories, check if there is a logical connection between them.
- Use from_memory_index and to_memory_index (0-based indices into the memories array)."#);
        } else {
            prompt.push_str(r#",
  "relations": []"#);
        }

        prompt.push_str(r#"
}

Rules:
- Extract atomic, standalone facts. Each memory must be self-contained and express EXACTLY ONE idea.
- CRITICAL: If the input contains multiple facts, numbered lists, or compound statements joined by "and"/"also"/"additionally", you MUST split them into separate memories. Example: "I like Rust and Python" → two memories: "I like Rust" and "I like Python".
- Never merge or consolidate distinct pieces of information into a single memory. More granular = better.
- Use ALL 7 memory_type values when appropriate. Do not collapse skill/achievement into fact/experience.
- "skilled at", "can", "able to", "expert in" → always "skill".
- "achieved", "built", "completed", "finished", "won" → always "achievement".
- When uncertain between two types, prefer the more specific one (skill > fact, achievement > experience).
- Extract entities for EVERY named thing: people, tools, languages, frameworks, systems, projects.
- If you see causal or logical connections between extracted memories (cause→effect, evidence→conclusion, contradiction), you MUST include them in the "relations" array."#);

        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extraction_result_serialization() {
        let result = ExtractionResult {
            memories: vec![ExtractedMemory {
                text: "User prefers Rust".to_string(),
                memory_type: "preference".to_string(),
                certainty: 90,
                importance: 70,
                entities: vec!["rust".to_string()],
                context: Some("work".to_string()),
            }],
            entities: vec![ExtractedEntity {
                id: "rust".to_string(),
                name: "Rust".to_string(),
                entity_type: "concept".to_string(),
                relations: None,
            }],
            relations: vec![],
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("preference"));
    }
}
