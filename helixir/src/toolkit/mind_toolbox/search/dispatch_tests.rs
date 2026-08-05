use super::*;

fn row(id: &str, score: f32, flashback: bool) -> UnifiedSearchResult {
    let mut metadata = std::collections::HashMap::new();
    if flashback {
        metadata.insert("flashback".to_string(), serde_json::Value::Bool(true));
    }
    UnifiedSearchResult {
        memory_id: id.to_string(),
        internal_id: None,
        content: format!("content {id}"),
        score,
        method: "test".to_string(),
        metadata,
        created_at: String::new(),
        user_count: None,
        controversy: None,
    }
}

#[test]
fn flashbacks_never_crowd_in_window_rows() {
    // 4 in-window + 5 flashbacks, limit 4, allowance 2: all 4 in-window
    // rows survive; only 2 best flashbacks append after them.
    let mut input: Vec<UnifiedSearchResult> =
        (0..4).map(|i| row(&format!("in{i}"), 0.9, false)).collect();
    input.extend((0..5).map(|i| row(&format!("fb{i}"), 0.95, true)));
    let out = clamp_with_flashbacks(input, 4, 2);
    assert_eq!(out.len(), 6);
    assert!(out[..4].iter().all(|r| r.memory_id.starts_with("in")));
    assert!(out[4..].iter().all(|r| r.memory_id.starts_with("fb")));
}

#[test]
fn zero_allowance_drops_all_flashbacks() {
    let input = vec![row("a", 0.9, false), row("f", 0.8, true)];
    let out = clamp_with_flashbacks(input, 5, 0);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].memory_id, "a");
}

#[test]
fn dedup_still_runs_before_the_clamp() {
    // The same memory as seed AND expansion child must not eat a slot.
    let input = vec![
        row("dup", 0.9, false),
        row("dup", 0.7, false),
        row("b", 0.6, false),
    ];
    let out = clamp_with_flashbacks(input, 2, 0);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].memory_id, "dup");
    assert_eq!(out[1].memory_id, "b");
}

#[test]
fn superseded_window_accepts_internal_overfetch_above_sixty() {
    assert_eq!(superseded_window(100, 250), 100);
    assert_eq!(superseded_window(100, 40), 40);
}

#[test]
fn superseded_window_keeps_the_existing_small_limit_cap() {
    assert_eq!(superseded_window(10, 100), 30);
    assert_eq!(superseded_window(30, 100), 60);
}
