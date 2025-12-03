use regex::Regex;
use std::collections::{HashMap, HashSet};
use lazy_static::lazy_static;
use super::models::{SearchResult, SearchMethod};

lazy_static! {
    static ref STOPWORDS: HashSet<&'static str> = {
        let mut set = HashSet::new();
        set.extend([
            "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of", 
            "with", "by", "from", "as", "is", "was", "are", "were", "be", "been", "being"
        ]);
        set
    };
    static ref WORD_REGEX: Regex = Regex::new(r"\b\w+\b").unwrap();
}

pub struct Bm25Search;

impl Bm25Search {
    pub fn tokenize(text: &str) -> Vec<String> {
        let lower = text.to_lowercase();
        WORD_REGEX
            .find_iter(&lower)
            .map(|m| m.as_str().to_string())
            .filter(|token| !STOPWORDS.contains(token.as_str()) && token.len() > 2)
            .collect()
    }

    pub fn calculate_score(
        query_tokens: &[String],
        doc_tokens: &[String],
        avg_doc_length: f64,
        k1: f64,
        b: f64,
    ) -> f64 {
        if query_tokens.is_empty() || doc_tokens.is_empty() {
            return 0.0;
        }

        let doc_length = doc_tokens.len() as f64;
        let mut score = 0.0;

        let mut doc_tf: HashMap<&str, i32> = HashMap::new();
        for token in doc_tokens {
            *doc_tf.entry(token).or_insert(0) += 1;
        }

        for query_term in query_tokens {
            if let Some(&tf) = doc_tf.get(query_term.as_str()) {
                let tf = tf as f64;
                let numerator = tf * (k1 + 1.0);
                let denominator = tf + k1 * (1.0 - b + b * (doc_length / avg_doc_length));
                score += numerator / denominator;
            }
        }

        if !query_tokens.is_empty() {
            score /= query_tokens.len() as f64;
        }

        score.min(1.0)
    }

    pub fn search(
        query: &str,
        documents: &[(String, String)],
        limit: usize,
        min_score: f64,
    ) -> Vec<SearchResult> {
        if documents.is_empty() {
            return Vec::new();
        }

        let query_tokens = Self::tokenize(query);
        if query_tokens.is_empty() {
            return Vec::new();
        }

        let doc_tokens: Vec<Vec<String>> = documents
            .iter()
            .map(|(_, content)| Self::tokenize(content))
            .collect();

        let total_length: f64 = doc_tokens.iter().map(|tokens| tokens.len() as f64).sum();
        let avg_doc_length = total_length / documents.len() as f64;

        let mut results: Vec<SearchResult> = documents
            .iter()
            .zip(doc_tokens.iter())
            .filter_map(|((memory_id, content), tokens)| {
                let score = Self::calculate_score(&query_tokens, tokens, avg_doc_length, 1.5, 0.75);
                if score >= min_score {
                    Some(SearchResult {
                        memory_id: memory_id.clone(),
                        content: content.clone(),
                        score,
                        method: SearchMethod::Bm25,
                        metadata: HashMap::new(),
                        created_at: String::new(),
                    })
                } else {
                    None
                }
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        results.truncate(limit);
        results
    }
}