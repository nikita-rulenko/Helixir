use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::providers::base::{LlmProvider, LlmProviderError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub memories: Vec<ExtractedMemory>,

    // Weak-model tolerance (observed live from the fallback tier): a missing
    // top-level `entities`/`relations` array must not nuke the WHOLE
    // extraction into the blob fallback — atoms without entities are still
    // atoms.
    #[serde(default)]
    pub entities: Vec<ExtractedEntity>,

    #[serde(default)]
    pub relations: Vec<ExtractedRelation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedMemory {
    pub text: String,

    #[serde(default = "default_memory_type")]
    pub memory_type: String,

    #[serde(default = "default_certainty_i32", deserialize_with = "lenient_i32")]
    pub certainty: i32,

    #[serde(default = "default_importance_i32", deserialize_with = "lenient_i32")]
    pub importance: i32,

    /// Weak models emit `["e1"]`, `[{"id":"e1",...}]` or omit the field —
    /// all observed live; all accepted.
    #[serde(default, deserialize_with = "lenient_string_vec")]
    pub entities: Vec<String>,

    #[serde(default, deserialize_with = "lenient_opt_string")]
    pub context: Option<String>,
}

fn default_memory_type() -> String {
    "fact".to_string()
}
fn default_entity_type() -> String {
    "Object".to_string()
}
fn default_certainty_i32() -> i32 {
    70
}
fn default_importance_i32() -> i32 {
    50
}

/// Accept an int, a float (0..=1 scales to 0..=100), or a numeric string.
fn lenient_i32<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i32, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i as i32
            } else if let Some(f) = n.as_f64() {
                if f <= 1.0 {
                    (f * 100.0) as i32
                } else {
                    f as i32
                }
            } else {
                70
            }
        }
        serde_json::Value::String(s) => s.trim().parse::<f64>().map_or(70, |f| {
            if f <= 1.0 {
                (f * 100.0) as i32
            } else {
                f as i32
            }
        }),
        _ => 70,
    })
}

/// Accept `["a"]`, `[{"id":"a"}]`, `[{"name":"a"}]`, a bare `"a"`, or null.
fn lenient_string_vec<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    let coerce = |item: &serde_json::Value| -> Option<String> {
        match item {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(o) => o
                .get("id")
                .or_else(|| o.get("name"))
                .and_then(|x| x.as_str())
                .map(str::to_string),
            _ => None,
        }
    };
    Ok(match &v {
        serde_json::Value::Array(a) => a.iter().filter_map(coerce).collect(),
        serde_json::Value::String(_) => coerce(&v).into_iter().collect(),
        _ => Vec::new(),
    })
}

/// Accept a string, an object (take `name`/`value`), or anything else → None.
fn lenient_opt_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::String(s) => Some(s),
        serde_json::Value::Object(o) => o
            .get("name")
            .or_else(|| o.get("value"))
            .and_then(|x| x.as_str())
            .map(str::to_string),
        _ => None,
    })
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
    #[serde(default)]
    pub id: String,

    pub name: String,

    #[serde(
        rename = "type",
        alias = "entity_type",
        default = "default_entity_type"
    )]
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

fn default_strength() -> i32 {
    80
}
fn default_strength_i64() -> i64 {
    80
}
fn default_confidence() -> i32 {
    80
}

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
            preview, user_id
        );

        let system_prompt = self.build_system_prompt(extract_entities, extract_relations);
        let user_prompt = format!("Extract information from this text:\n\n{}", text);

        let (response, _metadata) = self
            .provider
            .generate(&system_prompt, &user_prompt, Some("json_object"))
            .await?;

        match self.try_parse_extraction(&response) {
            Ok(result) if !result.memories.is_empty() => {
                debug!(
                    "Extracted {} memories, {} entities, {} relations",
                    result.memories.len(),
                    result.entities.len(),
                    result.relations.len()
                );
                Ok(result)
            }
            first_attempt => {
                let first_err = match &first_attempt {
                    Ok(r) if r.memories.is_empty() => "0 memories extracted".to_string(),
                    Err(e) => format!("parse error: {}", e),
                    _ => unreachable!(),
                };
                warn!(
                    "Extraction attempt 1 failed ({}), retrying with stricter prompt",
                    first_err
                );

                let retry_prompt = format!(
                    "{}\n\nIMPORTANT: Your previous response was invalid ({}). Output ONLY valid JSON matching the schema. No markdown fences, no explanation text outside JSON.",
                    user_prompt, first_err
                );

                match self
                    .provider
                    .generate(&system_prompt, &retry_prompt, Some("json_object"))
                    .await
                {
                    Ok((retry_response, _)) => match self.try_parse_extraction(&retry_response) {
                        Ok(result) if !result.memories.is_empty() => {
                            info!(
                                "Extraction retry succeeded: {} memories",
                                result.memories.len()
                            );
                            Ok(result)
                        }
                        Ok(_) => {
                            warn!("Extraction retry also returned 0 memories, using fallback");
                            Ok(self.fallback_extraction(text))
                        }
                        Err(e) => {
                            warn!("Extraction retry parse failed: {}, using fallback", e);
                            Ok(self.fallback_extraction(text))
                        }
                    },
                    Err(e) => {
                        warn!("Extraction retry LLM call failed: {}, using fallback", e);
                        Ok(self.fallback_extraction(text))
                    }
                }
            }
        }
    }

    fn try_parse_extraction(&self, response: &str) -> Result<ExtractionResult, String> {
        serde_json::from_str::<ExtractionResult>(response)
            .or_else(|_| {
                if let Some(start) = response.find('{') {
                    if let Some(end) = response.rfind('}') {
                        return serde_json::from_str(&response[start..=end])
                            .map_err(|e| e.to_string());
                    }
                }
                Err("no JSON object found in response".to_string())
            })
            .map_err(|e| e.to_string())
    }

    fn fallback_extraction(&self, text: &str) -> ExtractionResult {
        ExtractionResult {
            memories: vec![ExtractedMemory {
                text: text.to_string(),
                memory_type: "fact".to_string(),
                certainty: 50,
                importance: 50,
                entities: vec![],
                context: None,
            }],
            entities: vec![],
            relations: vec![],
        }
    }

    fn build_system_prompt(&self, extract_entities: bool, extract_relations: bool) -> String {
        let mut prompt = String::from(
            r#"You are a memory extraction system. Analyze the text and extract structured information.

Each extracted memory MUST have a memory_type from EXACTLY one of these 8 types:

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
- "action": A specific action performed, a task executed, or an operation carried out.
  Example: "I deployed the server" or "I ran the database migration"
  NOTE: "I did X", "I ran X", "I executed X", "I performed X" is ALWAYS an action, not an experience or fact.

Output JSON with this structure:
{
  "memories": [
    {
      "text": "self-contained informative statement with key context",
      "memory_type": "fact|preference|skill|goal|opinion|experience|achievement|action",
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
            prompt.push_str(
                r#",
  "entities": []"#,
            );
        }

        if extract_relations {
            prompt.push_str(
                r#",
  "relations": [
    {
      "from_memory_index": 0,
      "to_memory_index": 1,
      "relation_type": "IMPLIES|BECAUSE|CONTRADICTS|SUPPORTS|RELATES_TO|PART_OF|IS_A",
      "strength": 80,
      "confidence": 80,
      "explanation": "Why this relation exists"
    }
  ]"#,
            );
            prompt.push_str(
                r#"

Relations use INDICES into the memories array (0-based). This typed graph is the
whole point of the memory — for each related pair, pick the SINGLE most accurate
edge by this procedure.

DECISION PROCEDURE (check in order, STOP at the first match):
1. Does A cause or explain B?            -> BECAUSE (A is the cause) or IMPLIES (A leads to B)
2. Is A evidence/justification for B?     -> SUPPORTS
3. Do A and B conflict?                   -> CONTRADICTS
4. Is A a COMPONENT / PART / MEMBER of B? -> PART_OF   (wheel PART_OF car; lexer PART_OF compiler)
5. Is A a KIND / TYPE / INSTANCE of B?    -> IS_A      (dog IS_A animal; Rust IS_A language)
6. Same topic, none of the above?         -> RELATES_TO
SUPPORTS is ONLY evidence for a claim — NEVER for components (use PART_OF) or
categories (use IS_A). Do NOT default to IMPLIES or SUPPORTS.

EXAMPLES (the pair -> the correct edge):
- "The wheel is round" / "The car has four wheels"        -> PART_OF
- "Python is interpreted" / "Python is a programming language" -> IS_A
- "The cache stores sessions" / "The cache uses Redis"    -> RELATES_TO
- "Tests passed" / "The build is green"                   -> SUPPORTS
- "The server crashed" / "The disk was full"              -> BECAUSE

MANDATORY: if the input contains an explicit causal connective (because,
therefore, so, since, потому что, так как, из-за, поэтому), you MUST split it
into the cause atom and the effect atom AND emit the BECAUSE edge between
them (cause -> effect). A causal sentence with no BECAUSE edge is an error.

Emit an edge for EVERY related pair, even with only 2 memories. Use
from_memory_index and to_memory_index (0-based).

WORKED EXAMPLE — copy this shape EXACTLY (every field, plain strings in
memory "entities", ALL top-level keys present even when empty):

Input: "The deploy failed because the auth token expired. I use ArgoCD."
Output:
{
  "memories": [
    {"text": "The deploy failed on the release pipeline", "memory_type": "action", "certainty": 85, "importance": 60, "entities": ["argocd"], "context": "work"},
    {"text": "The auth token expired", "memory_type": "fact", "certainty": 85, "importance": 55, "entities": [], "context": "work"},
    {"text": "I use ArgoCD for deployments", "memory_type": "fact", "certainty": 90, "importance": 50, "entities": ["argocd"], "context": "work"}
  ],
  "entities": [
    {"id": "argocd", "name": "ArgoCD", "type": "system"}
  ],
  "relations": [
    {"from_memory_index": 1, "to_memory_index": 0, "relation_type": "BECAUSE", "strength": 85, "confidence": 85, "explanation": "the expired token is the cause of the failed deploy"}
  ]
}"#,
            );
        } else {
            prompt.push_str(
                r#",
  "relations": []"#,
            );
        }

        prompt.push_str(r#"
}

Rules:
- Each memory must be SELF-CONTAINED and INFORMATIVE — a reader must understand it without seeing the original text.
- Preserve key context: names, numbers, versions, dates, relationships. BAD: "X is a test". GOOD: "Integration test TestProductCRUD validates CRUD operations against SQLite".
- Split ONLY when the input contains genuinely distinct topics. Do NOT split a single coherent statement into trivial fragments.
- Example of GOOD splitting: "I like Rust for systems and Python for ML" → two memories with context.
- Example of BAD splitting: "The Eiffel Tower is in Paris" → do NOT split into "The Eiffel Tower exists" + "The Eiffel Tower is in Paris".
- Aim for 1-3 memories per input sentence. Fewer, richer memories are better than many trivial ones.
- Use ALL 8 memory_type values when appropriate. Do not collapse skill/achievement into fact/experience, and do not collapse action into experience.
- "skilled at", "can", "able to", "expert in" → always "skill".
- "achieved", "built", "completed", "finished", "won" → always "achievement".
- When uncertain between two types, prefer the more specific one (skill > fact, achievement > experience).
- Extract entities for EVERY named thing: people, tools, languages, frameworks, systems, projects.
- If you see causal or logical connections between extracted memories (cause→effect, evidence→conclusion, contradiction), you MUST include them in the "relations" array.

STRUCTURAL DATA PRESERVATION:
- If the input describes an API endpoint, include the HTTP method, path, and handler: "POST /api/v1/products is handled by ProductHandler.Create"
- If the input lists tests, include test names AND what they verify: "TestIntegrationProductCRUD validates create/read/update/delete operations for Product entities in SQLite"
- If the input describes code architecture (entities, layers, interfaces), preserve the full chain: "Product entity has fields: ID, Name, Price, CategoryID; repository interface defines CRUD + Search methods"
- If the input describes dependency relationships, preserve the chain: "Handler depends on Usecase, Usecase depends on Repository, Repository depends on Entity"
- NEVER drop structural details (field names, method signatures, endpoint paths, test lists) — these are critical for technical queries"#);

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

#[cfg(test)]
mod tolerance_tests {
    use super::*;

    /// The two failures observed live from the fallback tier on 2026-07-02:
    /// a missing top-level `entities` array, and objects where strings were
    /// expected inside `memories[].entities`. Both must parse, not fall back
    /// to the blob path.
    #[test]
    fn parses_weak_model_json_shapes() {
        // Shape 1: no top-level entities/relations at all.
        let r: ExtractionResult = serde_json::from_str(
            r#"{"memories":[{"text":"the deploy failed","memory_type":"fact","certainty":80,"importance":50,"entities":[]}]}"#,
        )
        .expect("missing top-level arrays must default");
        assert_eq!(r.memories.len(), 1);
        assert!(r.entities.is_empty() && r.relations.is_empty());

        // Shape 2: entity OBJECTS inside memory.entities + float certainty +
        // context as an object + missing memory_type.
        let r: ExtractionResult = serde_json::from_str(
            r#"{"memories":[{"text":"the token expired","certainty":0.9,"importance":"60","entities":[{"id":"e1","name":"token"}],"context":{"name":"auth"}}],"entities":[{"name":"token"}],"relations":[]}"#,
        )
        .expect("weak-model shapes must parse");
        let m = &r.memories[0];
        assert_eq!(m.memory_type, "fact");
        assert_eq!(m.certainty, 90);
        assert_eq!(m.importance, 60);
        assert_eq!(m.entities, vec!["e1".to_string()]);
        assert_eq!(m.context.as_deref(), Some("auth"));
        assert_eq!(r.entities[0].entity_type, "Object");
    }
}
