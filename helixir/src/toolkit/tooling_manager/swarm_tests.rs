use super::*;

fn at(ts: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .unwrap()
        .with_timezone(&chrono::Utc)
}

fn presence(last_seen: &str) -> AgentPresence {
    AgentPresence {
        agent_id: "a".into(),
        principal_id: "a".into(),
        name: "a".into(),
        role: "developer".into(),
        host: "h".into(),
        last_seen: last_seen.into(),
        status: "idle".into(),
    }
}

#[test]
fn active_within_window_stale_outside() {
    let now = at("2026-06-16T12:00:00Z");
    let p = presence("2026-06-16T11:59:30Z");
    assert_eq!(p.age_seconds(now), Some(30));
    assert!(p.is_active(now, 90));
    assert!(!p.is_active(now, 10));
}

#[test]
fn never_seen_is_not_active() {
    let now = at("2026-06-16T12:00:00Z");
    let p = presence("");
    assert_eq!(p.age_seconds(now), None);
    assert!(!p.is_active(now, 90));
}

#[test]
fn future_heartbeat_is_not_active() {
    let now = at("2026-06-16T12:00:00Z");
    assert!(!presence("2026-06-16T12:05:00Z").is_active(now, 90));
}

#[test]
fn farewell_status_is_immediately_inactive_inside_window() {
    let now = at("2026-06-16T12:00:00Z");
    let mut p = presence("2026-06-16T12:00:00Z");
    p.status = "done".into();
    assert_eq!(p.age_seconds(now), Some(0));
    assert!(!p.is_active(now, 90));
}

#[test]
fn descriptive_non_terminal_status_remains_active() {
    let now = at("2026-06-16T12:00:00Z");
    let mut p = presence("2026-06-16T11:59:59Z");
    p.status = "testing v0.15 control plane".into();
    assert!(p.is_active(now, 90));
}

#[test]
fn has_agent_id_handles_both_shapes() {
    assert!(has_agent_id(
        &serde_json::json!({"agent": {"agent_id": "x"}})
    ));
    assert!(has_agent_id(&serde_json::json!({"agent_id": "x"})));
    assert!(!has_agent_id(
        &serde_json::json!({"agent": {"agent_id": ""}})
    ));
    assert!(!has_agent_id(&serde_json::json!({"agent": null})));
    assert!(!has_agent_id(&serde_json::json!({})));
}

#[test]
fn stored_principal_is_explicit_and_never_guessed_from_prefix() {
    assert_eq!(
        stored_principal_id(&serde_json::json!({
            "agent": {"agent_id": "codex-task", "principal_id": "codex"}
        })),
        Some("codex")
    );
    assert_eq!(
        stored_principal_id(&serde_json::json!({"agent_id": "codex-task"})),
        None
    );
}

#[test]
fn only_an_empty_first_lookup_is_treated_as_an_absent_agent() {
    assert!(is_missing_agent_lookup("Graph error: No value found"));
    assert!(is_missing_agent_lookup("GRAPH ERROR: NO VALUE FOUND"));
    assert!(!is_missing_agent_lookup("connection refused"));
    assert!(!is_missing_agent_lookup("invalid response payload"));
}

#[test]
fn families_count_logical_principals_and_concurrent_instances_separately() {
    let now = at("2026-06-16T12:00:00Z");
    let mut first = presence("2026-06-16T11:59:30Z");
    first.agent_id = "codex-build".into();
    first.principal_id = "codex".into();
    let mut second = presence("2026-06-16T11:59:40Z");
    second.agent_id = "codex-review".into();
    second.principal_id = "codex".into();
    second.status = "done".into();
    let mut third = presence("2026-06-16T11:59:20Z");
    third.agent_id = "claude-research".into();
    third.principal_id = "claude".into();

    let families = aggregate_agent_families(&[first, second, third], now, 90);
    assert_eq!(families.len(), 2);
    let codex = families
        .iter()
        .find(|family| family.principal_id == "codex")
        .unwrap();
    assert_eq!(codex.total_instances, 2);
    assert_eq!(codex.active_instances, 1);
    assert!(codex.active);
}

#[test]
fn legacy_instances_join_the_longest_known_family_for_display() {
    let now = at("2026-06-16T12:00:00Z");
    let mut legacy = presence("2026-06-16T11:59:30Z");
    legacy.agent_id = "codex-web-build".into();
    legacy.principal_id.clear();
    let known = ["codex".to_string(), "codex-web".to_string()]
        .into_iter()
        .collect();
    normalize_legacy_agent_principals(std::slice::from_mut(&mut legacy), &known);

    assert_eq!(legacy.principal_id, "codex-web");
    let families = aggregate_agent_families(&[legacy], now, 90);
    assert_eq!(families[0].principal_id, "codex-web");
}
