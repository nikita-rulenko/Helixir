use crate::commands::check::run;
use crate::config::{DbConfig, HelixConfig, LocalInstanceConfig};
use crate::metrics_sender::MetricsSender;
use crate::tests::test_utils::TestContext;
use std::fs;
use std::path::PathBuf;

/// Helper to create a metrics sender for tests
fn create_test_metrics_sender() -> MetricsSender {
    MetricsSender::new().expect("Failed to create metrics sender")
}

/// This test requires cloning the helix repo which is slow and requires network.
/// Run with: cargo test --package helix-cli -- --ignored --test-threads=1
#[tokio::test]
#[ignore]
async fn test_check_all_instances_success() {
    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);
    let metrics_sender = create_test_metrics_sender();

    let result = run(None, &metrics_sender).await;
    assert!(
        result.is_ok(),
        "Check should succeed with valid project: {:?}",
        result.err()
    );
}

/// This test requires cloning the helix repo which is slow and requires network.
/// Run with: cargo test --package helix-cli -- --ignored --test-threads=1
#[tokio::test]
#[ignore]
async fn test_check_specific_instance_success() {
    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);
    let metrics_sender = create_test_metrics_sender();

    let result = run(Some("dev".to_string()), &metrics_sender).await;
    assert!(
        result.is_ok(),
        "Check should succeed for valid instance: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_check_nonexistent_instance_fails() {
    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);
    let metrics_sender = create_test_metrics_sender();

    let result = run(Some("nonexistent".to_string()), &metrics_sender).await;
    assert!(
        result.is_err(),
        "Check should fail for nonexistent instance"
    );
    let error_msg = result.err().unwrap().to_string();
    assert!(
        error_msg.contains("not found") || error_msg.contains("nonexistent"),
        "Error should mention instance not found"
    );
}

#[tokio::test]
async fn test_check_fails_without_schema() {
    let ctx = TestContext::new();
    ctx.setup_project_without_schema();

    let _guard = std::env::set_current_dir(&ctx.project_path);
    let metrics_sender = create_test_metrics_sender();

    let result = run(None, &metrics_sender).await;
    assert!(result.is_err(), "Check should fail without schema");
    let error_msg = result.err().unwrap().to_string();
    assert!(
        error_msg.contains("schema") || error_msg.contains("N::") || error_msg.contains("E::"),
        "Error should mention missing schema definitions"
    );
}

#[tokio::test]
async fn test_check_fails_with_invalid_syntax() {
    let ctx = TestContext::new();
    ctx.setup_project_with_invalid_syntax();

    let _guard = std::env::set_current_dir(&ctx.project_path);
    let metrics_sender = create_test_metrics_sender();

    let result = run(None, &metrics_sender).await;
    assert!(result.is_err(), "Check should fail with invalid syntax");
}

#[tokio::test]
async fn test_check_fails_without_helix_toml() {
    let ctx = TestContext::new();
    // Don't set up any project - leave directory empty

    let _guard = std::env::set_current_dir(&ctx.project_path);
    let metrics_sender = create_test_metrics_sender();

    let result = run(None, &metrics_sender).await;
    assert!(
        result.is_err(),
        "Check should fail without helix.toml in project"
    );
    let error_msg = result.err().unwrap().to_string();
    assert!(
        error_msg.contains("not found") || error_msg.contains("helix.toml"),
        "Error should mention missing helix.toml"
    );
}

/// This test requires cloning the helix repo which is slow and requires network.
/// Run with: cargo test --package helix-cli -- --ignored --test-threads=1
#[tokio::test]
#[ignore]
async fn test_check_with_multiple_instances() {
    let ctx = TestContext::new();

    // Create helix.toml with multiple instances
    let mut config = HelixConfig::default_config("test-project");
    config.local.insert(
        "staging".to_string(),
        LocalInstanceConfig {
            port: Some(6970),
            build_mode: crate::config::BuildMode::Dev,
            db_config: DbConfig::default(),
        },
    );
    config.local.insert(
        "production".to_string(),
        LocalInstanceConfig {
            port: Some(6971),
            build_mode: crate::config::BuildMode::Dev,
            db_config: DbConfig::default(),
        },
    );
    let config_path = ctx.project_path.join("helix.toml");
    config
        .save_to_file(&config_path)
        .expect("Failed to save config");

    // Create .helix directory
    fs::create_dir_all(ctx.project_path.join(".helix")).expect("Failed to create .helix");

    // Create valid queries and schema
    let queries_dir = ctx.project_path.join("db");
    fs::create_dir_all(&queries_dir).expect("Failed to create queries directory");

    let schema_content = r#"
N::User {
    name: String,
}

E::Follows {
    From: User,
    To: User,
}
"#;
    fs::write(queries_dir.join("schema.hx"), schema_content).expect("Failed to write schema.hx");

    let _guard = std::env::set_current_dir(&ctx.project_path);
    let metrics_sender = create_test_metrics_sender();

    let result = run(None, &metrics_sender).await;
    assert!(
        result.is_ok(),
        "Check should succeed with multiple instances: {:?}",
        result.err()
    );
}

/// This test requires cloning the helix repo which is slow and requires network.
/// Run with: cargo test --package helix-cli -- --ignored --test-threads=1
#[tokio::test]
#[ignore]
async fn test_check_validates_each_instance_individually() {
    let ctx = TestContext::new();
    ctx.setup_valid_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);
    let metrics_sender = create_test_metrics_sender();

    // Check the specific instance
    let result = run(Some("dev".to_string()), &metrics_sender).await;
    assert!(result.is_ok(), "Check should validate dev instance");
}

#[tokio::test]
async fn test_check_with_empty_queries_directory() {
    let ctx = TestContext::new();

    // Create helix.toml
    let config = HelixConfig::default_config("test-project");
    let config_path = ctx.project_path.join("helix.toml");
    config
        .save_to_file(&config_path)
        .expect("Failed to save config");

    // Create .helix directory
    fs::create_dir_all(ctx.project_path.join(".helix")).expect("Failed to create .helix");

    // Create queries directory but leave it empty
    let queries_dir = ctx.project_path.join("db");
    fs::create_dir_all(&queries_dir).expect("Failed to create queries directory");

    let _guard = std::env::set_current_dir(&ctx.project_path);
    let metrics_sender = create_test_metrics_sender();

    let result = run(None, &metrics_sender).await;
    assert!(
        result.is_err(),
        "Check should fail with empty queries directory"
    );
}

/// This test requires cloning the helix repo which is slow and requires network.
/// Run with: cargo test --package helix-cli -- --ignored --test-threads=1
#[tokio::test]
#[ignore]
async fn test_check_with_schema_only() {
    let ctx = TestContext::new();
    ctx.setup_schema_only_project();

    let _guard = std::env::set_current_dir(&ctx.project_path);
    let metrics_sender = create_test_metrics_sender();

    let result = run(None, &metrics_sender).await;
    assert!(
        result.is_ok(),
        "Check should succeed with schema only (queries are optional): {:?}",
        result.err()
    );
}

/// This test requires cloning the helix repo which is slow and requires network.
/// Run with: cargo test --package helix-cli -- --ignored --test-threads=1
#[tokio::test]
#[ignore]
async fn test_check_with_multiple_hx_files() {
    let ctx = TestContext::new();

    // Create helix.toml
    let config = HelixConfig::default_config("test-project");
    let config_path = ctx.project_path.join("helix.toml");
    config
        .save_to_file(&config_path)
        .expect("Failed to save config");

    // Create .helix directory
    fs::create_dir_all(ctx.project_path.join(".helix")).expect("Failed to create .helix");

    // Create queries directory
    let queries_dir = ctx.project_path.join("db");
    fs::create_dir_all(&queries_dir).expect("Failed to create queries directory");

    // Create schema in one file
    let schema_content = r#"
N::User {
    name: String,
}
"#;
    fs::write(queries_dir.join("schema.hx"), schema_content).expect("Failed to write schema.hx");

    // Create additional schema in another file
    let more_schema = r#"
N::Post {
    title: String,
}

E::Authored {
    From: User,
    To: Post,
}
"#;
    fs::write(queries_dir.join("more_schema.hx"), more_schema)
        .expect("Failed to write more_schema.hx");

    // Create queries in yet another file
    let queries = r#"
QUERY GetUser(id: ID) =>
    user <- N<User>(id)
    RETURN user
"#;
    fs::write(queries_dir.join("queries.hx"), queries).expect("Failed to write queries.hx");

    let _guard = std::env::set_current_dir(&ctx.project_path);
    let metrics_sender = create_test_metrics_sender();

    let result = run(None, &metrics_sender).await;
    assert!(
        result.is_ok(),
        "Check should succeed with multiple .hx files: {:?}",
        result.err()
    );
}

/// This test requires cloning the helix repo which is slow and requires network.
/// Run with: cargo test --package helix-cli -- --ignored --test-threads=1
#[tokio::test]
#[ignore]
async fn test_check_with_custom_queries_path() {
    let ctx = TestContext::new();

    // Create helix.toml with custom queries path
    let mut config = HelixConfig::default_config("test-project");
    config.project.queries = PathBuf::from("custom/helix/queries");
    let config_path = ctx.project_path.join("helix.toml");
    config
        .save_to_file(&config_path)
        .expect("Failed to save config");

    // Create .helix directory
    fs::create_dir_all(ctx.project_path.join(".helix")).expect("Failed to create .helix");

    // Create custom queries directory
    let queries_dir = ctx.project_path.join("custom/helix/queries");
    fs::create_dir_all(&queries_dir).expect("Failed to create custom queries directory");

    let schema_content = r#"
N::User {
    name: String,
}
"#;
    fs::write(queries_dir.join("schema.hx"), schema_content).expect("Failed to write schema.hx");

    let _guard = std::env::set_current_dir(&ctx.project_path);
    let metrics_sender = create_test_metrics_sender();

    let result = run(None, &metrics_sender).await;
    assert!(
        result.is_ok(),
        "Check should work with custom queries path: {:?}",
        result.err()
    );
}
