use std::collections::HashMap;
use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    pub static ref INTENT_PATTERNS: HashMap<&'static str, Vec<&'static str>> = {
        let mut m = HashMap::new();
        m.insert("preference", vec![
            r"\b(like|love|prefer|favorite|enjoy|fond of|into)\b",
            r"\b(what do i like|what are my favorites|my preferences)\b",
        ]);
        m.insert("skill", vec![
            r"\b(can i|able to|know how|capable|proficient|skilled|expert)\b",
            r"\b(what (can|do) i (do|know)|my (skills|abilities|expertise))\b",
        ]);
        m.insert("goal", vec![
            r"\b(want to|goal|plan|aim|intend|aspire|wish)\b",
            r"\b(my (goals|plans|objectives|ambitions))\b",
        ]);
        m.insert("fact", vec![
            r"\b(what is|tell me about|explain|describe|information about)\b",
            r"\b(how does|how do|what does)\b",
        ]);
        m.insert("opinion", vec![
            r"\b(think|believe|opinion|feel about|view on)\b",
            r"\b(what do i think|my (opinion|view|thoughts))\b",
        ]);
        m.insert("experience", vec![
            r"\b(did|have i|was i|when did|remember when)\b",
            r"\b(my (experience|history) with)\b",
        ]);
        m.insert("recent", vec![
            r"\b(today|yesterday|recently|lately|just now|this week)\b",
            r"\b(what (did|have) i (do|done)|current|latest)\b",
        ]);
        m
    };
}

lazy_static! {
    pub static ref EXPANSION_MAPPINGS: HashMap<&'static str, Vec<&'static str>> = {
        let mut m = HashMap::new();
        m.insert("like", vec!["love", "enjoy", "prefer", "fond of", "appreciate"]);
        m.insert("love", vec!["like", "adore", "enjoy", "passionate about"]);
        m.insert("prefer", vec!["like", "favor", "choose", "opt for"]);
        m.insert("can", vec!["able to", "capable of", "know how to", "proficient in"]);
        m.insert("skill", vec!["ability", "expertise", "competence", "proficiency"]);
        m.insert("want", vec!["wish", "desire", "aim", "plan", "intend"]);
        m.insert("goal", vec!["objective", "target", "aim", "ambition", "plan"]);
        m.insert("python", vec!["programming", "coding", "development", "backend"]);
        m.insert("ai", vec!["artificial intelligence", "machine learning", "ml", "llm"]);
        m.insert("today", vec!["now", "current", "recent", "latest"]);
        m.insert("recently", vec!["lately", "just", "new", "fresh"]);
        m
    };
}

pub fn intent_to_concept(intent: &str) -> Option<&'static str> {
    match intent {
        "preference" => Some("Preference"),
        "skill" => Some("Skill"),
        "goal" => Some("Goal"),
        "fact" => Some("Fact"),
        "opinion" => Some("Opinion"),
        "experience" => Some("Experience"),
        "recent" => None,
        _ => None,
    }
}

pub fn detect_intent(query: &str) -> Vec<&'static str> {
    let mut detected_intents = Vec::new();
    
    for (intent, patterns) in INTENT_PATTERNS.iter() {
        for pattern in patterns {
            if let Ok(regex) = Regex::new(pattern) {
                if regex.is_match(query) {
                    detected_intents.push(*intent);
                    break;
                }
            }
        }
    }
    
    detected_intents
}

pub fn expand_query(query: &str) -> String {
    let mut expanded = query.to_string();
    
    for (term, synonyms) in EXPANSION_MAPPINGS.iter() {
        for synonym in synonyms {
            expanded = expanded.replace(synonym, term);
        }
    }
    
    expanded
}