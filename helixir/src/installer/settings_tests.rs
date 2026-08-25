use super::*;

#[test]
fn debug_never_exposes_write_only_secrets() {
    let patch = SettingsPatch {
        reasoning_api_key: Some("reasoning-secret".into()),
        embedding_api_key: Some("embedding-secret".into()),
        ..SettingsPatch::default()
    };
    let rendered = format!("{patch:?}");
    assert!(!rendered.contains("reasoning-secret"));
    assert!(!rendered.contains("embedding-secret"));
}

#[test]
fn unsafe_thresholds_and_provider_names_are_rejected() {
    assert!(
        validate(&SettingsPatch {
            reasoning_provider: Some("unknown".into()),
            ..SettingsPatch::default()
        })
        .is_err()
    );
    assert!(
        validate(&SettingsPatch {
            watchdog_mem_alert_pct: Some(90.0),
            watchdog_mem_restart_pct: Some(80.0),
            ..SettingsPatch::default()
        })
        .is_err()
    );
}

#[test]
fn effective_cross_field_constraints_include_existing_values() {
    let mut current = HelixirConfig::default();
    current.swarm.active_window_secs = 120;
    current.swarm.presence_ttl_secs = 300;
    current.watchdog.mem_alert_pct = 80.0;
    current.watchdog.mem_restart_pct = 95.0;

    assert!(
        validate_effective(
            &SettingsPatch {
                swarm_presence_ttl_secs: Some(60),
                ..SettingsPatch::default()
            },
            &current
        )
        .is_err()
    );
    assert!(
        validate_effective(
            &SettingsPatch {
                watchdog_mem_alert_pct: Some(97.0),
                ..SettingsPatch::default()
            },
            &current
        )
        .is_err()
    );
}

#[test]
fn generated_patch_contains_only_allowlisted_paths() {
    let patch = build_config_patch(&SettingsPatch {
        mode: Some(MemoryMode::Insights),
        backup_keep: Some(14),
        gateway_public_url: Some("https://memory.example.test/mcp".into()),
        ..SettingsPatch::default()
    });
    assert_eq!(patch.values.len(), 3);
    assert_eq!(patch.values["mode"], "Insights");
    assert_eq!(patch.values["watchdog.backup_keep"], "14");
    assert_eq!(
        patch.values["gateway.public_url"],
        "https://memory.example.test/mcp"
    );
}

#[test]
fn gateway_public_url_requires_http_transport() {
    assert!(
        validate(&SettingsPatch {
            gateway_public_url: Some("ssh://memory.example.test".into()),
            ..SettingsPatch::default()
        })
        .is_err()
    );
}
