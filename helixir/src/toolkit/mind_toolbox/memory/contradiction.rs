use std::collections::HashSet;

pub struct ContradictionDetector;

impl ContradictionDetector {
    pub fn detect_contradiction(old_content: &str, new_content: &str) -> bool {
        Self::get_contradiction_reason(old_content, new_content).is_some()
    }

    pub fn get_contradiction_reason(old_content: &str, new_content: &str) -> Option<String> {
        let old_lower = old_content.to_lowercase();
        let new_lower = new_content.to_lowercase();

        
        let negation_words = vec![
            "not", "never", "don't", "doesn't", "isn't", "aren't", 
            "wasn't", "weren't", "no longer", "actually", "but", "however", "instead"
        ];

        let old_words: HashSet<&str> = old_lower.split_whitespace().collect();
        
        for word in &negation_words {
            if new_lower.contains(word) && !old_words.contains(word) {
                return Some(format!("Negation detected: '{}'", word));
            }
        }

        
        let sentiment_pairs = vec![
            ("love", "hate"),
            ("best", "worst"),
            ("prefer", "avoid"),
        ];

        for (positive, negative) in sentiment_pairs {
            if (old_lower.contains(positive) && new_lower.contains(negative)) ||
               (old_lower.contains(negative) && new_lower.contains(positive)) {
                return Some(format!("Opposite sentiment: {} vs {}", positive, negative));
            }
        }

        
        let contradiction_markers = vec!["actually", "but", "however", "instead"];
        for marker in contradiction_markers {
            if new_lower.contains(marker) && !old_lower.contains(marker) {
                return Some(format!("Explicit contradiction marker: '{}'", marker));
            }
        }

        None
    }
}