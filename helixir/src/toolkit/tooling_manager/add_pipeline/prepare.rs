//! Extracted-memory hygiene: split incoherent texts (e.g. "X but Y") into
//! independent atoms so later decisions don't collapse contradictory clauses
//! into a single `UPDATE`.

use tracing::{debug, warn};

use crate::llm::extractor::ExtractedMemory;

use super::super::ToolingManager;

impl ToolingManager {
    pub(super) fn prepare_memories_for_storage(
        memories: Vec<ExtractedMemory>,
        message: &str,
    ) -> Vec<ExtractedMemory> {
        if memories.is_empty() {
            debug!("No memories extracted, storing original message");
            return vec![ExtractedMemory {
                text: message.to_string(),
                memory_type: "fact".to_string(),
                certainty: 50,
                importance: 50,
                entities: vec![],
                context: None,
            }];
        }

        let mut result = Vec::with_capacity(memories.len());
        for mem in memories {
            if Self::is_coherent_memory(&mem.text) {
                result.push(mem);
            } else {
                warn!(
                    "Splitting incoherent memory: {}...",
                    &mem.text.chars().take(60).collect::<String>()
                );
                let parts = Self::split_incoherent_memory(&mem);
                result.extend(parts);
            }
        }
        result
    }

    pub(super) fn is_coherent_memory(text: &str) -> bool {
        let contradiction_markers = [
            " but ",
            " however ",
            " although ",
            " whereas ",
            " on the other hand ",
            " in contrast ",
            " conversely ",
            " nevertheless ",
        ];
        let lower = text.to_lowercase();

        let sentence_count = text
            .split(|c: char| c == '.' || c == '!' || c == '?')
            .filter(|s| s.trim().len() > 10)
            .count();

        if sentence_count <= 1 {
            return true;
        }

        let has_contradiction = contradiction_markers.iter().any(|m| lower.contains(m));
        if !has_contradiction {
            return true;
        }

        let distinct_subjects = Self::count_distinct_subjects(&lower);
        if distinct_subjects <= 1 {
            return true;
        }

        false
    }

    fn count_distinct_subjects(text: &str) -> usize {
        let subject_indicators: Vec<&str> = text
            .split(|c: char| c == '.' || c == ';' || c == ',')
            .filter(|s| s.trim().len() > 5)
            .filter_map(|s| {
                let trimmed = s.trim();
                trimmed.split_whitespace().next()
            })
            .collect();

        let mut unique = std::collections::HashSet::new();
        for s in &subject_indicators {
            unique.insert(s.to_lowercase());
        }
        unique.len()
    }

    fn split_incoherent_memory(mem: &ExtractedMemory) -> Vec<ExtractedMemory> {
        let split_patterns = [
            " but ",
            " however ",
            " although ",
            " whereas ",
            " on the other hand ",
        ];
        let lower = mem.text.to_lowercase();

        for pattern in &split_patterns {
            if let Some(pos) = lower.find(pattern) {
                let part1 = mem.text[..pos].trim().to_string();
                let part2 = mem.text[pos + pattern.len()..].trim().to_string();

                if part1.len() > 10 && part2.len() > 10 {
                    return vec![
                        ExtractedMemory {
                            text: part1,
                            memory_type: mem.memory_type.clone(),
                            certainty: mem.certainty,
                            importance: mem.importance,
                            entities: mem.entities.clone(),
                            context: mem.context.clone(),
                        },
                        ExtractedMemory {
                            text: part2,
                            memory_type: mem.memory_type.clone(),
                            certainty: mem.certainty,
                            importance: mem.importance,
                            entities: mem.entities.clone(),
                            context: mem.context.clone(),
                        },
                    ];
                }
            }
        }

        vec![mem.clone()]
    }
}
