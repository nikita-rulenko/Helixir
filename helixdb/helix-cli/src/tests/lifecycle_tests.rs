//! Tests for lifecycle commands (start, stop, restart, status, delete)
//!
//! These tests focus on error paths and project configuration validation
//! that don't require Docker to be actually running.

use crate::commands::{restart, start, status, stop};
use crate::config::HelixConfig;
use crate::tests::test_utils::TestContext;
use std::fs;

// ============================================================================
// Start Command Tests
// ============================================================================

#[tokio::test]
async fn test_start_fails_without_helix_project() {
    let ctx = TestContext::new();
    // Don't set up any project

    let _guard = std::env::set_current_dir(&ctx.project_path);

    let result = start::run(Some("dev".to_string())).await;
    assert!(
        result.is_err(),
        "Start should fail when not in a helix project"
    );
    let error_msg = format!("{:?}", result.err().unwrap());
    assert!(
        error_msg.contains("not found") || error_msg.contains("helix.toml"),
        "Error should mention missing project configuration"
    );
}

#[tokio::test]
async fn test_start_fails_with_nonexistent_instance() {
    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);

    let result = start::run(Some("nonexistent".to_string())).await;
    assert!(
        result.is_err(),
        "Start should fail for nonexistent instance"
    );
    let error_msg = format!("{:?}", result.err().unwrap());
    assert!(
        error_msg.contains("not found") || error_msg.contains("nonexistent"),
        "Error should mention instance not found"
    );
}

#[tokio::test]
async fn test_start_fails_without_build_artifacts() {
    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);

    // dev instance exists in config but has not been built
    let result = start::run(Some("dev".to_string())).await;

    // This should fail because docker-compose.yml doesn't exist
    // (either due to not being built OR Docker not being available)
    assert!(result.is_err(), "Start should fail without build artifacts");
    let error_msg = format!("{:?}", result.err().unwrap());
    // Could fail due to missing build OR missing Docker
    assert!(
        error_msg.contains("built")
            || error_msg.contains("docker")
            || error_msg.contains("Docker")
            || error_msg.contains("not found"),
        "Error should indicate missing build or Docker: {error_msg}"
    );
}

#[tokio::test]
async fn test_start_requires_instance_when_non_interactive() {
    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);

    // In non-interactive mode, start without instance should fail
    // because prompts::is_interactive() returns false in tests
    let result = start::run(None).await;

    // This should fail asking for an instance
    assert!(result.is_err(), "Start should fail without instance name");
    let error_msg = format!("{:?}", result.err().unwrap());
    assert!(
        error_msg.contains("No instance") || error_msg.contains("instance"),
        "Error should mention no instance specified"
    );
}

// ============================================================================
// Stop Command Tests
// ============================================================================

#[tokio::test]
async fn test_stop_fails_without_helix_project() {
    let ctx = TestContext::new();
    // Don't set up any project

    let _guard = std::env::set_current_dir(&ctx.project_path);

    let result = stop::run(Some("dev".to_string())).await;
    assert!(
        result.is_err(),
        "Stop should fail when not in a helix project"
    );
}

#[tokio::test]
async fn test_stop_fails_with_nonexistent_instance() {
    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);

    let result = stop::run(Some("nonexistent".to_string())).await;
    assert!(result.is_err(), "Stop should fail for nonexistent instance");
    let error_msg = format!("{:?}", result.err().unwrap());
    assert!(
        error_msg.contains("not found") || error_msg.contains("nonexistent"),
        "Error should mention instance not found"
    );
}

#[tokio::test]
async fn test_stop_requires_instance_when_non_interactive() {
    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);

    let result = stop::run(None).await;
    assert!(result.is_err(), "Stop should fail without instance name");
    let error_msg = format!("{:?}", result.err().unwrap());
    assert!(
        error_msg.contains("No instance") || error_msg.contains("instance"),
        "Error should mention no instance specified"
    );
}

// ============================================================================
// Restart Command Tests
// ============================================================================

#[tokio::test]
async fn test_restart_fails_without_helix_project() {
    let ctx = TestContext::new();
    // Don't set up any project

    let _guard = std::env::set_current_dir(&ctx.project_path);

    let result = restart::run(Some("dev".to_string())).await;
    assert!(
        result.is_err(),
        "Restart should fail when not in a helix project"
    );
}

#[tokio::test]
async fn test_restart_fails_with_nonexistent_instance() {
    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);

    let result = restart::run(Some("nonexistent".to_string())).await;
    assert!(
        result.is_err(),
        "Restart should fail for nonexistent instance"
    );
    let error_msg = format!("{:?}", result.err().unwrap());
    assert!(
        error_msg.contains("not found") || error_msg.contains("nonexistent"),
        "Error should mention instance not found"
    );
}

#[tokio::test]
async fn test_restart_fails_without_build_artifacts() {
    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);

    // dev instance exists but has not been built
    let result = restart::run(Some("dev".to_string())).await;

    // This should fail because docker-compose.yml doesn't exist
    assert!(
        result.is_err(),
        "Restart should fail without build artifacts"
    );
    let error_msg = format!("{:?}", result.err().unwrap());
    // Could fail due to missing build OR missing Docker
    assert!(
        error_msg.contains("built")
            || error_msg.contains("docker")
            || error_msg.contains("Docker")
            || error_msg.contains("not found"),
        "Error should indicate missing build or Docker: {error_msg}"
    );
}

#[tokio::test]
async fn test_restart_requires_instance_when_non_interactive() {
    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);

    let result = restart::run(None).await;
    assert!(result.is_err(), "Restart should fail without instance name");
    let error_msg = format!("{:?}", result.err().unwrap());
    assert!(
        error_msg.contains("No instance") || error_msg.contains("instance"),
        "Error should mention no instance specified"
    );
}

// ============================================================================
// Status Command Tests
// ============================================================================

#[tokio::test]
async fn test_status_handles_no_project_gracefully() {
    let ctx = TestContext::new();
    // Don't set up any project

    let _guard = std::env::set_current_dir(&ctx.project_path);

    // Status command should handle missing project gracefully (not panic)
    let result = status::run().await;
    // Status should succeed but print an error message
    assert!(result.is_ok(), "Status should not panic without project");
}

#[tokio::test]
async fn test_status_shows_project_info() {
    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);

    // Status command should succeed when in a valid project
    let result = status::run().await;
    // Status will try to check Docker which may not be available,
    // but it should still succeed by handling Docker errors gracefully
    assert!(result.is_ok(), "Status should succeed in valid project");
}

#[tokio::test]
async fn test_status_with_multiple_instances() {
    let ctx = TestContext::new();

    // Create project with multiple instances
    let mut config = HelixConfig::default_config("multi-instance-project");
    config.local.insert(
        "staging".to_string(),
        crate::config::LocalInstanceConfig {
            port: Some(6970),
            build_mode: crate::config::BuildMode::Dev,
            db_config: crate::config::DbConfig::default(),
        },
    );
    config.local.insert(
        "production".to_string(),
        crate::config::LocalInstanceConfig {
            port: Some(6971),
            build_mode: crate::config::BuildMode::Release,
            db_config: crate::config::DbConfig::default(),
        },
    );

    config
        .save_to_file(&ctx.project_path.join("helix.toml"))
        .expect("Failed to save config");
    fs::create_dir_all(ctx.project_path.join(".helix")).expect("Failed to create .helix");

    let _guard = std::env::set_current_dir(&ctx.project_path);

    let result = status::run().await;
    assert!(
        result.is_ok(),
        "Status should succeed with multiple instances"
    );
}

// ============================================================================
// Delete Command Tests (limited - requires confirmation)
// ============================================================================

#[tokio::test]
async fn test_delete_fails_with_nonexistent_instance_dev() {
    use crate::commands::delete;

    let ctx = TestContext::new();

    // Create helix.toml but clear all instances - this prevents the test from
    // walking up directories and finding another test's helix.toml
    let mut config = HelixConfig::default_config("test-project");
    config.local.clear(); // Remove the default "dev" instance
    config
        .save_to_file(&ctx.project_path.join("helix.toml"))
        .expect("Failed to save config");
    fs::create_dir_all(ctx.project_path.join(".helix")).expect("Failed to create .helix");

    let _guard = std::env::set_current_dir(&ctx.project_path);

    let result = delete::run("dev".to_string()).await;
    assert!(
        result.is_err(),
        "Delete should fail when instance doesn't exist"
    );
}

#[tokio::test]
async fn test_delete_fails_with_nonexistent_instance() {
    use crate::commands::delete;

    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);

    let result = delete::run("nonexistent".to_string()).await;
    assert!(
        result.is_err(),
        "Delete should fail for nonexistent instance"
    );
    let error_msg = format!("{:?}", result.err().unwrap());
    assert!(
        error_msg.contains("not found") || error_msg.contains("nonexistent"),
        "Error should mention instance not found"
    );
}

// ============================================================================
// Instance Configuration Tests
// ============================================================================

#[tokio::test]
async fn test_instance_validation_rejects_empty_name() {
    let _ctx = TestContext::new();

    // Create config manually with validation
    let config = HelixConfig::default_config("test-project");

    // The get_instance method should fail for empty string
    let result = config.get_instance("");
    assert!(
        result.is_err(),
        "get_instance should fail for empty instance name"
    );
}

#[tokio::test]
async fn test_project_context_finds_helix_toml() {
    use crate::project::ProjectContext;

    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let result = ProjectContext::find_and_load(Some(&ctx.project_path));
    assert!(
        result.is_ok(),
        "ProjectContext should find helix.toml: {:?}",
        result.err()
    );

    let project = result.unwrap();
    assert_eq!(project.config.project.name, "test-project");
}

#[tokio::test]
async fn test_project_context_fails_without_helix_toml() {
    use crate::project::ProjectContext;

    let ctx = TestContext::new();
    // Don't create helix.toml

    let result = ProjectContext::find_and_load(Some(&ctx.project_path));
    assert!(
        result.is_err(),
        "ProjectContext should fail without helix.toml"
    );
}
