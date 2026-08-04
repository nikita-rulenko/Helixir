use super::*;

/// Proves a code path makes no LLM call: panics if consulted.
struct PanicProvider;

#[async_trait::async_trait]
impl crate::llm::providers::base::LlmProvider for PanicProvider {
    async fn generate(
        &self,
        _system_prompt: &str,
        _user_prompt: &str,
        _response_format: Option<&str>,
    ) -> Result<
        (String, crate::llm::providers::base::LlmMetadata),
        crate::llm::providers::base::LlmProviderError,
    > {
        panic!("LLM must not be consulted on a gated decision");
    }

    fn provider_name(&self) -> &str {
        "panic"
    }

    fn model_name(&self) -> &str {
        "panic"
    }
}

fn gated_engine() -> LLMDecisionEngine {
    LLMDecisionEngine::with_thresholds(
        std::sync::Arc::new(PanicProvider)
            as std::sync::Arc<dyn crate::llm::providers::base::LlmProvider>,
        0.70,
        0.98,
    )
}

fn similar(id: &str, score: f64) -> SimilarMemory {
    SimilarMemory {
        id: id.to_string(),
        content: "same fact".to_string(),
        score,
        memory_type: None,
        created_at: None,
        user_id: None,
        is_cross_user: false,
    }
}

#[tokio::test]
async fn cosine_gates_skip_llm() {
    let engine = gated_engine();

    // Upper gate: near-verbatim duplicate -> NOOP, no LLM.
    let d = engine
        .decide("the same fact", "fact", &[similar("mem_dup", 0.99)], "u")
        .await;
    assert_eq!(d.operation, MemoryOperation::Noop);
    assert_eq!(d.target_memory_id.as_deref(), Some("mem_dup"));

    // Lower gate: nothing above the similarity threshold -> ADD, no LLM.
    let d = engine
        .decide("a novel fact", "fact", &[similar("mem_far", 0.42)], "u")
        .await;
    assert_eq!(d.operation, MemoryOperation::Add);

    // No candidates at all -> ADD, no LLM.
    let d = engine.decide("first fact ever", "fact", &[], "u").await;
    assert_eq!(d.operation, MemoryOperation::Add);

    // Exact string match -> NOOP regardless of the (blended) score.
    let d = engine
        .decide("same fact", "fact", &[similar("mem_same", 0.80)], "u")
        .await;
    assert_eq!(d.operation, MemoryOperation::Noop);
    assert_eq!(d.target_memory_id.as_deref(), Some("mem_same"));

    // Protected types never cosine-gate: a 0.99 "duplicate" preference
    // may be a one-word reversal — must reach the LLM (gray zone).
    let gray = engine.gate(
        "the user prefers light theme",
        "preference",
        &[similar("mem_pref", 0.99)],
    );
    assert!(gray.is_err(), "protected type must not be cosine-gated");
}

#[test]
fn test_memory_decision_builders() {
    let add = MemoryDecision::add(100, "test reason");
    assert_eq!(add.operation, MemoryOperation::Add);
    assert_eq!(add.confidence, 100);

    let noop = MemoryDecision::noop(90, "duplicate");
    assert_eq!(noop.operation, MemoryOperation::Noop);

    let update = MemoryDecision::update("mem_123", "merged", 85, "merging");
    assert_eq!(update.operation, MemoryOperation::Update);
    assert_eq!(update.target_memory_id, Some("mem_123".to_string()));
    assert_eq!(update.merged_content, Some("merged".to_string()));

    let supersede = MemoryDecision::supersede("mem_old", 80, "evolved");
    assert_eq!(supersede.operation, MemoryOperation::Supersede);
    assert_eq!(supersede.supersedes_memory_id, Some("mem_old".to_string()));
}

#[test]
fn test_link_existing_builder() {
    let link = MemoryDecision::link_existing("mem_shared", 90, "same fact from different user");
    assert_eq!(link.operation, MemoryOperation::LinkExisting);
    assert_eq!(link.link_to_memory_id, Some("mem_shared".to_string()));
    assert_eq!(link.confidence, 90);
    assert!(link.target_memory_id.is_none());
    assert!(link.conflict_type.is_none());
}

#[test]
fn test_cross_contradict_builder() {
    let cc =
        MemoryDecision::cross_contradict("mem_other", "preference", 85, "opposing preferences");
    assert_eq!(cc.operation, MemoryOperation::CrossContradict);
    assert_eq!(cc.contradicts_memory_id, Some("mem_other".to_string()));
    assert_eq!(cc.conflict_type, Some("preference".to_string()));
    assert_eq!(cc.confidence, 85);
    assert!(cc.link_to_memory_id.is_none());
}

#[test]
fn test_similar_memory_cross_user_fields() {
    let personal = SimilarMemory {
        id: "mem_1".to_string(),
        content: "test".to_string(),
        score: 0.9,
        memory_type: None,
        created_at: None,
        user_id: None,
        is_cross_user: false,
    };
    assert!(!personal.is_cross_user);

    let cross = SimilarMemory {
        id: "mem_2".to_string(),
        content: "test".to_string(),
        score: 0.85,
        memory_type: None,
        created_at: None,
        user_id: Some("other_user".to_string()),
        is_cross_user: true,
    };
    assert!(cross.is_cross_user);
    assert_eq!(cross.user_id, Some("other_user".to_string()));
}

#[test]
fn test_prompt_includes_cross_user_section() {
    use super::super::prompt::build_decision_prompt;

    let cross_memories = vec![SimilarMemory {
        id: "mem_other".to_string(),
        content: "I prefer dark mode".to_string(),
        score: 0.88,
        memory_type: None,
        created_at: Some("2025-01-01T00:00:00Z".to_string()),
        user_id: Some("user_b".to_string()),
        is_cross_user: true,
    }];

    let prompt = build_decision_prompt("I prefer light mode", &cross_memories, "user_a");

    assert!(prompt.contains("LINK_EXISTING"));
    assert!(prompt.contains("CROSS_CONTRADICT"));
    assert!(prompt.contains("DIFFERENT USER"));
    assert!(prompt.contains("link_to_memory_id"));
}

#[test]
fn test_prompt_no_cross_user_section_for_personal() {
    use super::super::prompt::build_decision_prompt;

    let personal_memories = vec![SimilarMemory {
        id: "mem_mine".to_string(),
        content: "Rust is my favorite language".to_string(),
        score: 0.9,
        memory_type: None,
        created_at: None,
        user_id: None,
        is_cross_user: false,
    }];

    let prompt = build_decision_prompt("Rust is great", &personal_memories, "user_a");

    assert!(!prompt.contains("LINK_EXISTING"));
    assert!(!prompt.contains("CROSS_CONTRADICT"));
    assert!(!prompt.contains("DIFFERENT USER"));
}

#[test]
fn test_prompt_keeps_rbac_outside_curation() {
    use super::super::prompt::{SYSTEM_PROMPT, build_batch_decision_prompt, build_decision_prompt};

    let prompt = build_decision_prompt("Rust is my language", &[], "user_a");
    assert!(SYSTEM_PROMPT.contains("RBAC boundary"));
    assert!(prompt.contains("user_id"));
    assert!(prompt.contains("authorization is handled by `RbacManager`"));

    let batch = build_batch_decision_prompt(&[(0, "Rust is my language", &[])], "user_a");
    assert!(batch.contains("Cross-user similarity/linkage never grants visibility"));
}

/// #96 Lever 1.5: a scripted provider returning queued responses, with a
/// call counter — proves batch reliability without a live LLM.
struct ScriptedProvider {
    responses: std::sync::Mutex<Vec<String>>,
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl crate::llm::providers::base::LlmProvider for ScriptedProvider {
    async fn generate(
        &self,
        _system_prompt: &str,
        _user_prompt: &str,
        _response_format: Option<&str>,
    ) -> Result<
        (String, crate::llm::providers::base::LlmMetadata),
        crate::llm::providers::base::LlmProviderError,
    > {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let mut q = self.responses.lock().unwrap();
        let body = if q.is_empty() {
            panic!("ScriptedProvider exhausted — more LLM calls than the contract allows")
        } else {
            q.remove(0)
        };
        Ok((body, crate::llm::providers::base::LlmMetadata::default()))
    }

    fn provider_name(&self) -> &str {
        "scripted"
    }

    fn model_name(&self) -> &str {
        "scripted"
    }
}

fn scripted_engine(responses: Vec<&str>) -> (LLMDecisionEngine, Arc<ScriptedProvider>) {
    let provider = Arc::new(ScriptedProvider {
        responses: std::sync::Mutex::new(responses.into_iter().map(String::from).collect()),
        calls: AtomicUsize::new(0),
    });
    (
        LLMDecisionEngine::with_thresholds(
            Arc::clone(&provider) as Arc<dyn crate::llm::providers::base::LlmProvider>,
            0.70,
            0.98,
        ),
        provider,
    )
}

fn batch_items(specs: &[(&str, f64)]) -> Vec<(String, String, Vec<SimilarMemory>)> {
    specs
        .iter()
        .enumerate()
        .map(|(n, (text, score))| {
            (
                (*text).to_string(),
                "fact".to_string(),
                vec![similar(&format!("mem_c{n}"), *score)],
            )
        })
        .collect()
}

/// Gating makes the original indices sparse (item 0 gated here), and the
/// model answers with DENSE indices 0..n-1 — exactly what used to dump
/// every item into a per-item call. One call must now resolve all three.
#[tokio::test]
async fn sparse_gray_indices_resolve_in_one_call() {
    let (engine, provider) = scripted_engine(vec![
        r#"{"decisions":[
                {"i":0,"operation":"ADD","confidence":80,"reasoning":"a"},
                {"i":1,"operation":"ADD","confidence":80,"reasoning":"b"},
                {"i":2,"operation":"UPDATE","target_memory_id":"mem_c3","confidence":85,"reasoning":"c","merged_content":"m"}
            ]}"#,
    ]);
    // Item 0 gated (exact dup), items 1..3 gray -> local indices 0..2.
    let items = batch_items(&[
        ("dup", 0.99),
        ("gray one", 0.85),
        ("gray two", 0.85),
        ("gray three", 0.85),
    ]);
    let decisions = engine.decide_batch(&items, "u").await;

    assert_eq!(
        provider.calls.load(Ordering::Relaxed),
        1,
        "one batched call"
    );
    assert_eq!(decisions[0].operation, MemoryOperation::Noop); // gated
    assert_eq!(decisions[1].operation, MemoryOperation::Add);
    assert_eq!(decisions[2].operation, MemoryOperation::Add);
    assert_eq!(decisions[3].operation, MemoryOperation::Update);
    assert_eq!(decisions[3].target_memory_id.as_deref(), Some("mem_c3"));
}

/// An incomplete first response is repaired by ONE batched retry — never
/// by N per-item calls (a per-item call would exhaust the script and
/// panic, and the call counter pins the total at 2).
#[tokio::test]
async fn incomplete_batch_repairs_in_one_batched_retry() {
    let (engine, provider) = scripted_engine(vec![
        r#"{"decisions":[{"i":0,"operation":"ADD","confidence":80,"reasoning":"only one"}]}"#,
        r#"{"decisions":[
                {"i":0,"operation":"ADD","confidence":80,"reasoning":"a"},
                {"i":1,"operation":"ADD","confidence":80,"reasoning":"b"},
                {"i":2,"operation":"ADD","confidence":80,"reasoning":"c"}
            ]}"#,
    ]);
    let items = batch_items(&[("g1", 0.85), ("g2", 0.85), ("g3", 0.85)]);
    let decisions = engine.decide_batch(&items, "u").await;

    assert_eq!(
        provider.calls.load(Ordering::Relaxed),
        2,
        "initial + one batched repair, zero per-item calls"
    );
    assert!(
        decisions
            .iter()
            .all(|d| d.operation == MemoryOperation::Add)
    );
}
