use super::*;
use crate::toolkit::fast_think::limits::FastThinkLimits;
use crate::toolkit::fast_think::models::{ThoughtEdge, ThoughtType};

/// Only recalls in the conclusion's supporting subtree are evidence — a
/// broad exploratory recall on an unrelated branch must NOT become
/// SUPPORTS provenance (the inflation observed live: ~105 edges).
#[test]
fn evidence_excludes_unrelated_recalls() {
    let limits = FastThinkLimits::default();
    let mut s = ThinkingSession::new("t", None);
    let root = s
        .add_thought("pick a policy", ThoughtType::Initial, None, None, &limits)
        .unwrap();
    let obs = s
        .add_thought(
            "outages are short",
            ThoughtType::Observation,
            Some(root),
            Some(ThoughtEdge::LeadsTo),
            &limits,
        )
        .unwrap();
    let used = s
        .add_recalled_thought("queue fact", "mem_used", 0.9, obs, &limits)
        .unwrap();
    let _ = used;
    // Unrelated branch with its own recall.
    let side = s
        .add_thought(
            "tangent",
            ThoughtType::Question,
            Some(root),
            Some(ThoughtEdge::LeadsTo),
            &limits,
        )
        .unwrap();
    s.add_recalled_thought("noise fact", "mem_noise", 0.9, side, &limits)
        .unwrap();

    s.add_conclusion("backoff with jitter", &[obs], &limits)
        .unwrap();

    let ev = s.get_conclusion_evidence_ids();
    assert!(
        ev.contains(&"mem_used".to_string()),
        "supporting recall kept: {ev:?}"
    );
    assert!(
        !ev.contains(&"mem_noise".to_string()),
        "unrelated recall must be excluded: {ev:?}"
    );
}

#[test]
fn bound_session_rejects_a_different_actor() {
    let s = ThinkingSession::new("secured", Some("alice"));
    assert!(s.authorize_actor(Some("alice")).is_ok());
    assert!(matches!(
        s.authorize_actor(Some("mallory")),
        Err(FastThinkError::Unauthorized)
    ));
    assert!(matches!(
        s.authorize_actor(None),
        Err(FastThinkError::Unauthorized)
    ));

    let legacy = ThinkingSession::new("legacy", None);
    assert!(legacy.authorize_actor(None).is_ok());
    assert!(matches!(
        legacy.authorize_actor(Some("alice")),
        Err(FastThinkError::Unauthorized)
    ));
}
