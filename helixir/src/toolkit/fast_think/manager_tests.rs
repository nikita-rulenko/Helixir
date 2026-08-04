use super::*;
use crate::core::config::HelixirConfig;

#[test]
fn hot_reload_pins_existing_sessions_and_updates_new_sessions() {
    let client =
        Arc::new(HelixirClient::new(HelixirConfig::default()).expect("test client constructs"));
    let old_limits = FastThinkLimits {
        max_thoughts: 11,
        ..FastThinkLimits::default()
    };
    let manager = FastThinkManager::new(Arc::clone(&client), old_limits);
    manager
        .start_thinking("old", "before reload", None)
        .expect("old session starts");

    let new_limits = FastThinkLimits {
        max_thoughts: 23,
        ..FastThinkLimits::default()
    };
    manager.update_runtime(client, new_limits);
    manager
        .start_thinking("new", "after reload", None)
        .expect("new session starts");

    assert_eq!(manager.session_max_thoughts("old", None).unwrap(), 11);
    assert_eq!(manager.session_max_thoughts("new", None).unwrap(), 23);
    assert_eq!(manager.max_thoughts(), 23);
}

#[test]
fn bound_session_rejects_cross_principal_lifecycle_operations() {
    let client =
        Arc::new(HelixirClient::new(HelixirConfig::default()).expect("test client constructs"));
    let manager = FastThinkManager::new(client, FastThinkLimits::default());
    manager
        .start_thinking("private", "secret analysis", Some("alice"))
        .expect("session starts");

    assert!(matches!(
        manager.get_session_status("private", Some("mallory")),
        Err(FastThinkError::Unauthorized)
    ));
    assert!(matches!(
        manager.add_thought(
            "private",
            Some("mallory"),
            "tamper",
            ThoughtType::Reasoning,
            None,
            None,
        ),
        Err(FastThinkError::Unauthorized)
    ));
    assert!(matches!(
        manager.discard("private", Some("mallory")),
        Err(FastThinkError::Unauthorized)
    ));

    let status = manager
        .get_session_status("private", Some("alice"))
        .expect("owner still retains session");
    assert_eq!(status.thought_count, 1);
}
