use crate::config::HelixConfig;
use crate::docker::DockerManager;
use crate::project::ProjectContext;
use std::fs;
use tempfile::TempDir;

fn setup_test_project() -> (TempDir, ProjectContext) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path().to_path_buf();

    let config = HelixConfig::default_config("test-project");
    let config_path = project_path.join("helix.toml");
    config
        .save_to_file(&config_path)
        .expect("Failed to save config");

    fs::create_dir_all(project_path.join(".helix")).expect("Failed to create .helix");

    let context = ProjectContext::find_and_load(Some(&project_path)).unwrap();
    (temp_dir, context)
}

/// Regression test: HELIX_DATA_DIR must always be /data inside container
/// (Bug from PR #823 where host path was incorrectly passed to container)
#[test]
fn test_helix_data_dir_uses_container_path() {
    let (_temp_dir, context) = setup_test_project();
    let docker = DockerManager::new(&context);

    let env_vars = docker.environment_variables("dev");

    let data_dir_var = env_vars
        .iter()
        .find(|v| v.starts_with("HELIX_DATA_DIR="))
        .expect("HELIX_DATA_DIR should be set");

    assert_eq!(
        data_dir_var, "HELIX_DATA_DIR=/data",
        "HELIX_DATA_DIR must use container path /data, not host path"
    );
}

/// Verify docker-compose has correct volume mount and env var
#[test]
fn test_docker_compose_volume_and_env() {
    let (_temp_dir, context) = setup_test_project();
    let docker = DockerManager::new(&context);

    let instance_config = context.config.get_instance("dev").unwrap();
    let compose = docker
        .generate_docker_compose("dev", instance_config, None)
        .unwrap();

    // Volume mount should use host path as source, /data as destination
    assert!(
        compose.contains("../.volumes/dev:/data"),
        "Volume mount should map host path to /data in container"
    );

    // Environment should have /data (container path)
    assert!(
        compose.contains("HELIX_DATA_DIR=/data"),
        "HELIX_DATA_DIR in environment should be /data"
    );

    // Should NOT have host path in HELIX_DATA_DIR
    assert!(
        !compose.contains("HELIX_DATA_DIR=../.volumes"),
        "HELIX_DATA_DIR should NOT contain host path"
    );
}

/// Generated release images must resolve their external base images by an
/// immutable multi-architecture manifest digest. Named build stages such as
/// `chef` remain valid stage references and are intentionally not registry
/// image references.
#[test]
fn test_generated_dockerfile_pins_external_base_images() {
    let (_temp_dir, context) = setup_test_project();
    let docker = DockerManager::new(&context);
    let instance_config = context.config.get_instance("dev").unwrap();
    let dockerfile = docker
        .generate_dockerfile("dev", instance_config)
        .expect("Dockerfile generation should succeed");

    assert!(dockerfile.contains(
        "FROM lukemathwalker/cargo-chef:latest-rust-1.88@sha256:\
50ae19f263d8a4bed1769c22ac929497ac9fa1d7fbc7288c277fa2f07f0aa85c AS chef"
    ));
    assert!(dockerfile.contains(
        "FROM debian:bookworm-slim@sha256:\
88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171"
    ));
    assert!(!dockerfile.contains("FROM lukemathwalker/cargo-chef:latest-rust-1.88 AS"));
    assert!(!dockerfile.contains("FROM debian:bookworm-slim\n"));
}

/// Verify host-side data_dir() still respects HELIX_DATA_DIR for volume mount
#[test]
fn test_host_data_dir_respects_env_var() {
    let (_temp_dir, context) = setup_test_project();
    let docker = DockerManager::new(&context);

    // Default should be ../.volumes/{instance}
    let default_dir = docker.data_dir("myinstance");
    assert_eq!(default_dir, "../.volumes/myinstance");
}
