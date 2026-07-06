//! #94 quality harness: EVERY ontology type must be usable — and used well.
//!
//! The live store shows the starvation this guards against: fact=69-85% of
//! all memories while preference/skill/experience sit at 0-3 rows per
//! thousand. A labeled corpus (2 sentences per type × 8 types, EN + RU) runs
//! through the REAL extractor prompt on the REAL LLM; each sentence is
//! unambiguous for a human, so misclassification is a prompt defect, not
//! corpus noise.
//!
//! Contract (variance-tolerant, starvation-intolerant):
//! - overall accuracy >= 75%
//! - NO type scores 0/2 — a fully-starved category is exactly the #94 bug.
//!
//! ```text
//! HELIX_E2E=1 cargo test -p helixir --test ontology_classification_e2e -- --ignored --nocapture
//! ```

use std::sync::Arc;

use helixir::core::HelixirConfig;
use helixir::llm::LlmProviderFactory;
use helixir::llm::extractor::LlmExtractor;

/// (expected_type, sentence) — one EN + one RU per type. Deliberately NOT
/// the prompt's worked examples (that would test memorization, and the
/// example-leak firewall exists precisely because models copy them).
const CORPUS: &[(&str, &str)] = &[
    ("fact", "The Volga is the longest river in Europe."),
    (
        "fact",
        "PostgreSQL хранит данные таблиц в страницах по 8 килобайт.",
    ),
    ("preference", "I prefer tea over coffee in the morning."),
    (
        "preference",
        "Мне больше нравится тёмная тема в редакторе, чем светлая.",
    ),
    (
        "skill",
        "I can tune HNSW indexes for recall without losing latency.",
    ),
    ("skill", "Я умею писать асинхронный код на Rust."),
    ("goal", "I want to ship the beta version by September."),
    ("goal", "Моя цель — выучить японский язык до конца года."),
    (
        "opinion",
        "I think microservices are overused in small teams.",
    ),
    (
        "opinion",
        "По-моему, ручное тестирование сильно недооценивают.",
    ),
    (
        "experience",
        "Reviewing the old logs, I realized the outage had started hours earlier than anyone reported.",
    ),
    (
        "experience",
        "Разбирая архив памяти, я осознал, что читаю заметки, оставленные прошлой версией меня.",
    ),
    (
        "achievement",
        "I finally shipped the compiler backend after six months of work.",
    ),
    ("achievement", "Я выиграл городской турнир по шахматам."),
    ("action", "I restarted the ingestion daemon at noon."),
    ("action", "Я развернул новую схему на тестовом сервере."),
];

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 + a working LLM (real extraction calls)"]
async fn every_ontology_type_is_classified_correctly() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );

    let config = HelixirConfig::from_env();
    let provider: Arc<dyn helixir::llm::LlmProvider> = Arc::from(LlmProviderFactory::create(
        &config.llm_provider,
        &config.llm_model,
        config.llm_api_key.as_deref(),
        config.llm_base_url.as_deref(),
        f64::from(config.llm_temperature),
        config.llm_runtime.request_timeout_secs,
    ));
    let extractor = LlmExtractor::new(provider);

    let mut per_type: std::collections::HashMap<&str, (u32, u32)> =
        std::collections::HashMap::new();
    let mut misses: Vec<String> = Vec::new();

    for (expected, sentence) in CORPUS {
        let result = extractor
            .extract(sentence, "ontology_probe", false, false)
            .await
            .unwrap_or_else(|e| panic!("extraction failed for {sentence:?}: {e}"));
        // The dominant type across extracted atoms: a single labeled sentence
        // may legitimately split into atoms, but its LABEL type must appear.
        let got: Vec<&str> = result
            .memories
            .iter()
            .map(|m| m.memory_type.as_str())
            .collect();
        let hit = got.contains(expected);
        let entry = per_type.entry(expected).or_insert((0, 0));
        entry.1 += 1;
        if hit {
            entry.0 += 1;
        } else {
            misses.push(format!("{expected}: {sentence:?} -> {got:?}"));
        }
    }

    let total: u32 = per_type.values().map(|(_, n)| n).sum();
    let correct: u32 = per_type.values().map(|(ok, _)| ok).sum();

    println!(
        "\n==== ontology_classification_e2e (llm={}) ====",
        config.llm_model
    );
    let mut types: Vec<_> = per_type.iter().collect();
    types.sort();
    for (t, (ok, n)) in &types {
        println!("  {t:<12} {ok}/{n}");
    }
    println!("  overall      {correct}/{total}");
    for m in &misses {
        println!("  MISS {m}");
    }

    assert!(
        correct * 4 >= total * 3,
        "ontology accuracy below 75%: {correct}/{total}\n{}",
        misses.join("\n")
    );
    for (t, (ok, _)) in &types {
        assert!(
            *ok > 0,
            "type '{t}' fully starved (0 correct) — the #94 bug:\n{}",
            misses.join("\n")
        );
    }
}

/// The starvation suspect the single-sentence test can't see: one MIXED
/// paragraph. Real agent writes are milestone paragraphs, and a classifier
/// that is perfect per-sentence may still collapse a paragraph's non-fact
/// sentences into `fact` atoms. The paragraph carries one unambiguous
/// sentence of each of 6 non-fact types — at least 4 distinct non-fact
/// types must survive extraction.
#[tokio::test]
#[ignore = "needs HELIX_E2E=1 + a working LLM (real extraction calls)"]
async fn mixed_paragraph_does_not_collapse_into_fact() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let config = HelixirConfig::from_env();
    let provider: Arc<dyn helixir::llm::LlmProvider> = Arc::from(LlmProviderFactory::create(
        &config.llm_provider,
        &config.llm_model,
        config.llm_api_key.as_deref(),
        config.llm_base_url.as_deref(),
        f64::from(config.llm_temperature),
        config.llm_runtime.request_timeout_secs,
    ));
    let extractor = LlmExtractor::new(provider);

    let paragraph = "Today I deployed the new ingestion daemon to the staging server. \
        I prefer rolling deploys over blue-green for this service. \
        I want to move it to production by Friday. \
        Watching the first live traffic, I realized our retry logic masks real errors. \
        I think the retry budget is too generous. \
        After three weeks of work, I finally got the p99 latency under 200ms.";

    let result = extractor
        .extract(paragraph, "ontology_probe", false, false)
        .await
        .expect("extraction failed");

    let mut got: Vec<(String, String)> = result
        .memories
        .iter()
        .map(|m| (m.memory_type.clone(), m.text.chars().take(60).collect()))
        .collect();
    got.sort();
    println!("\n==== mixed paragraph atoms ====");
    for (t, text) in &got {
        println!("  {t:<12} {text}");
    }

    let distinct_non_fact: std::collections::HashSet<&str> = result
        .memories
        .iter()
        .map(|m| m.memory_type.as_str())
        .filter(|t| *t != "fact")
        .collect();
    assert!(
        distinct_non_fact.len() >= 4,
        "paragraph collapsed toward fact: only {distinct_non_fact:?} non-fact types among {} atoms",
        result.memories.len()
    );
}
