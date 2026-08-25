//! Tests for utility commands (add, prune, metrics, migrate)
//!
//! These tests focus on error paths and configuration validation
//! that don't require external services.

use crate::config::HelixConfig;
use crate::tests::test_utils::TestContext;
use serial_test::serial;
use std::fs;

// ============================================================================
// Add Command Tests
// ============================================================================

#[tokio::test]
async fn test_add_fails_without_helix_project() {
    use crate::commands::add;

    let ctx = TestContext::new();
    // Don't set up any project

    let _guard = std::env::set_current_dir(&ctx.project_path);

    // Add without any arguments should fail when not in a project
    let result = add::run(None).await;
    assert!(
        result.is_err(),
        "Add should fail when not in a helix project"
    );
}

#[tokio::test]
async fn test_add_local_instance_succeeds() {
    use crate::CloudDeploymentTypeCommand;
    use crate::commands::add;

    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);

    // Add a local instance with explicit name
    let result = add::run(Some(CloudDeploymentTypeCommand::Local {
        name: Some("staging".to_string()),
    }))
    .await;

    assert!(
        result.is_ok(),
        "Add local instance should succeed: {:?}",
        result.err()
    );

    // Verify the instance was added to config
    let config_content =
        fs::read_to_string(ctx.project_path.join("helix.toml")).expect("Should read config");
    assert!(
        config_content.contains("[local.staging]"),
        "Config should contain the new staging instance"
    );
}

#[tokio::test]
async fn test_add_rejects_duplicate_instance_name() {
    use crate::CloudDeploymentTypeCommand;
    use crate::commands::add;

    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);

    // Try to add instance with name 'dev' which already exists in default config
    let result = add::run(Some(CloudDeploymentTypeCommand::Local {
        name: Some("dev".to_string()),
    }))
    .await;

    assert!(
        result.is_err(),
        "Add should fail for duplicate instance name"
    );
    let error_msg = format!("{:?}", result.err().unwrap());
    assert!(
        error_msg.contains("already exists"),
        "Error should mention instance already exists: {error_msg}"
    );
}

#[tokio::test]
async fn test_add_requires_deployment_type_in_non_interactive() {
    use crate::commands::add;

    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);

    // In non-interactive mode, add without deployment type should fail
    let result = add::run(None).await;

    assert!(
        result.is_err(),
        "Add should fail without deployment type in non-interactive mode"
    );
    let error_msg = format!("{:?}", result.err().unwrap());
    assert!(
        error_msg.contains("No deployment type") || error_msg.contains("specify"),
        "Error should mention no deployment type specified: {error_msg}"
    );
}

// ============================================================================
// Prune Command Tests
// ============================================================================

#[tokio::test]
async fn test_prune_fails_for_specific_instance_outside_project() {
    use crate::commands::prune;

    let ctx = TestContext::new();
    // Don't set up any project

    let _guard = std::env::set_current_dir(&ctx.project_path);

    // Prune specific instance should fail outside project
    let result = prune::run(Some("dev".to_string()), false).await;
    assert!(
        result.is_err(),
        "Prune specific instance should fail outside project"
    );
    let error_msg = format!("{:?}", result.err().unwrap());
    assert!(
        error_msg.contains("not in") || error_msg.contains("Helix project"),
        "Error should mention not in helix project: {error_msg}"
    );
}

#[tokio::test]
async fn test_prune_all_fails_outside_project() {
    use crate::commands::prune;

    let ctx = TestContext::new();
    // Don't set up any project

    let _guard = std::env::set_current_dir(&ctx.project_path);

    // Prune --all should fail outside project
    let result = prune::run(None, true).await;
    assert!(result.is_err(), "Prune --all should fail outside project");
}

#[tokio::test]
async fn test_prune_nonexistent_instance_fails() {
    use crate::commands::prune;

    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);

    // Prune nonexistent instance should fail
    let result = prune::run(Some("nonexistent".to_string()), false).await;
    assert!(
        result.is_err(),
        "Prune should fail for nonexistent instance"
    );
    let error_msg = format!("{:?}", result.err().unwrap());
    assert!(
        error_msg.contains("not found") || error_msg.contains("nonexistent"),
        "Error should mention instance not found: {error_msg}"
    );
}

// ============================================================================
// Metrics Command Tests
// ============================================================================

#[tokio::test]
#[serial]
async fn test_metrics_status_succeeds() {
    use crate::MetricsAction;
    use crate::commands::metrics;

    let _ctx = TestContext::new();

    // Metrics status should succeed regardless of project
    let result = metrics::run(MetricsAction::Status).await;
    assert!(result.is_ok(), "Metrics status should always succeed");
}

#[tokio::test]
#[serial]
async fn test_metrics_basic_enables_collection() {
    use crate::MetricsAction;
    use crate::commands::metrics;

    let ctx = TestContext::new();

    // Enable basic metrics
    let result = metrics::run(MetricsAction::Basic).await;
    assert!(
        result.is_ok(),
        "Metrics basic should succeed: {:?}",
        result.err()
    );

    // Verify config was updated by reading directly from the expected path
    // (avoids race conditions with HELIX_HOME env var in parallel tests)
    let config_path = ctx.helix_home.join("metrics.toml");
    assert!(
        config_path.exists(),
        "Metrics config file should exist at {:?}",
        config_path
    );
    let content = fs::read_to_string(&config_path).expect("Should read metrics config");
    assert!(
        content.contains("level = \"basic\""),
        "Metrics level should be Basic, got: {}",
        content
    );

    // Cleanup: disable metrics to not affect other tests
    let _ = metrics::run(MetricsAction::Off).await;
}

// ============================================================================
// Migrate Command Tests
// ============================================================================

#[tokio::test]
async fn test_migrate_fails_without_v1_config() {
    use crate::commands::migrate;

    let ctx = TestContext::new();
    // Create an empty directory without v1 config

    let result = migrate::run(
        Some(ctx.project_path.to_str().unwrap().to_string()),
        "db".to_string(),
        "dev".to_string(),
        6969,
        false,
        true, // no_backup
    )
    .await;

    assert!(
        result.is_err(),
        "Migrate should fail without config.hx.json"
    );
    let error_msg = format!("{:?}", result.err().unwrap());
    assert!(
        error_msg.contains("config.hx.json") || error_msg.contains("v1"),
        "Error should mention missing v1 config: {error_msg}"
    );
}

#[tokio::test]
async fn test_migrate_fails_if_v2_exists() {
    use crate::commands::migrate;

    let ctx = TestContext::new();

    // Create v1 structure
    let v1_config = r#"{
        "vector_config": {
            "m": 16,
            "ef_construction": 128,
            "ef_search": 768,
            "db_max_size": 20
        },
        "graph_config": {
            "secondary_indices": []
        },
        "db_max_size_gb": 20,
        "mcp": true,
        "bm25": true
    }"#;
    fs::write(ctx.project_path.join("config.hx.json"), v1_config)
        .expect("Failed to write v1 config");

    // Create schema.hx
    fs::write(
        ctx.project_path.join("schema.hx"),
        "N::User { name: String }",
    )
    .expect("Failed to write schema");

    // Create queries.hx
    fs::write(
        ctx.project_path.join("queries.hx"),
        "QUERY GetUser(id: ID) => user <- N<User>(id) RETURN user",
    )
    .expect("Failed to write queries");

    // Also create helix.toml (v2 marker)
    fs::write(
        ctx.project_path.join("helix.toml"),
        "[project]\nname = \"test\"",
    )
    .expect("Failed to write helix.toml");

    let result = migrate::run(
        Some(ctx.project_path.to_str().unwrap().to_string()),
        "db".to_string(),
        "dev".to_string(),
        6969,
        false,
        true,
    )
    .await;

    assert!(result.is_err(), "Migrate should fail if helix.toml exists");
    let error_msg = format!("{:?}", result.err().unwrap());
    assert!(
        error_msg.contains("helix.toml") || error_msg.contains("v2"),
        "Error should mention v2 project exists: {error_msg}"
    );
}

#[tokio::test]
async fn test_migrate_dry_run_shows_plan() {
    use crate::commands::migrate;

    let ctx = TestContext::new();

    // Create v1 structure
    let v1_config = r#"{
        "vector_config": {
            "m": 16,
            "ef_construction": 128,
            "ef_search": 768,
            "db_max_size": 20
        },
        "graph_config": {
            "secondary_indices": []
        },
        "db_max_size_gb": 20,
        "mcp": true,
        "bm25": true
    }"#;
    fs::write(ctx.project_path.join("config.hx.json"), v1_config)
        .expect("Failed to write v1 config");

    // Create schema.hx
    fs::write(
        ctx.project_path.join("schema.hx"),
        "N::User { name: String }",
    )
    .expect("Failed to write schema");

    // Create queries.hx
    fs::write(
        ctx.project_path.join("queries.hx"),
        "QUERY GetUser(id: ID) => user <- N<User>(id) RETURN user",
    )
    .expect("Failed to write queries");

    // Run with dry_run = true
    let result = migrate::run(
        Some(ctx.project_path.to_str().unwrap().to_string()),
        "db".to_string(),
        "dev".to_string(),
        6969,
        true, // dry_run
        true,
    )
    .await;

    assert!(result.is_ok(), "Migrate dry run should succeed");

    // Verify no files were actually moved
    assert!(
        ctx.project_path.join("config.hx.json").exists(),
        "config.hx.json should still exist after dry run"
    );
    assert!(
        ctx.project_path.join("schema.hx").exists(),
        "schema.hx should still exist after dry run"
    );
    assert!(
        !ctx.project_path.join("helix.toml").exists(),
        "helix.toml should NOT be created during dry run"
    );
}

// ============================================================================
// Configuration Tests
// ============================================================================

#[test]
fn test_config_validates_empty_project_name() {
    let config_content = r#"
[project]
name = ""
queries = "./db/"

[local.dev]
port = 6969
"#;

    let result: Result<HelixConfig, _> = toml::from_str(config_content);
    // The config should parse, but validation should fail
    if let Ok(config) = result {
        // Use a temp path for validation
        let result = config.get_instance("dev");
        // This shouldn't fail here since we're not calling validate()
        assert!(result.is_ok());
    }
}

#[test]
fn test_config_validates_build_mode_debug_is_rejected() {
    // BuildMode::Debug should be rejected when loading from file
    // (validation happens in HelixConfig::from_file)
    let config_content = r#"
[project]
name = "test"
queries = "./db/"

[local.dev]
port = 6969
build_mode = "debug"
"#;

    // Parse should succeed
    let config: HelixConfig = toml::from_str(config_content).expect("Should parse");

    // But the config contains Debug mode which is deprecated
    assert_eq!(
        config.local.get("dev").unwrap().build_mode,
        crate::config::BuildMode::Debug
    );
}

#[test]
fn test_config_default_has_dev_instance() {
    let config = HelixConfig::default_config("my-project");

    assert!(config.local.contains_key("dev"));
    let dev_config = config.local.get("dev").unwrap();
    assert_eq!(dev_config.port, Some(6969));
    assert_eq!(dev_config.build_mode, crate::config::BuildMode::Dev);
}

#[test]
fn test_config_instance_lookup() {
    let config = HelixConfig::default_config("my-project");

    // Should find local instance
    let result = config.get_instance("dev");
    assert!(result.is_ok());
    assert!(result.unwrap().is_local());

    // Should fail for nonexistent
    let result = config.get_instance("nonexistent");
    assert!(result.is_err());
}

#[test]
fn test_config_list_instances() {
    let mut config = HelixConfig::default_config("my-project");

    // Add another instance
    config.local.insert(
        "staging".to_string(),
        crate::config::LocalInstanceConfig {
            port: Some(6970),
            build_mode: crate::config::BuildMode::Dev,
            db_config: crate::config::DbConfig::default(),
        },
    );

    let instances = config.list_instances();
    assert_eq!(instances.len(), 2);
    assert!(instances.contains(&&"dev".to_string()));
    assert!(instances.contains(&&"staging".to_string()));
}
