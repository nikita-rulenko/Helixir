//! Persistent HelixDB/Docker adapter.
//!
//! The command builders are intentionally pure.  Applying them is a separate
//! concern, which lets `--dry-run`, tests, and a native UI share the exact same
//! backup/rollback contract.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// HelixDB CLI version compatible with this LMDB-era schema.
pub const HELIX_CLI_VERSION: &str = "2.3.5";
/// Read-only contract returned by the deployed query inventory.
pub const SCHEMA_CONTRACT_VERSION: &str = "helixir-rbac-default-onboarding-v1";

/// Managed backend specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendSpec {
    /// Hostname/address used for health and schema deployment.
    pub host: String,
    /// Docker container name.
    pub container: String,
    /// Local image produced by HelixDB v2.3.5.
    pub image: String,
    /// Persistent Docker volume.
    pub volume: String,
    /// Published HTTP port.
    pub port: u16,
    /// Directory containing schema.hx and queries.hx.
    pub schema_dir: PathBuf,
    /// Directory containing the distributable `helix.toml` project manifest.
    pub project_dir: PathBuf,
}

impl Default for BackendSpec {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            container: "helixdb".to_string(),
            image: "helix-helixir-dev:latest".to_string(),
            volume: "helixdb_data".to_string(),
            port: crate::DEFAULT_HELIX_PORT,
            schema_dir: PathBuf::from("schema"),
            project_dir: PathBuf::from("."),
        }
    }
}

/// Shell-free Docker operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerCommand {
    /// Docker subcommand arguments, excluding `docker`.
    pub args: Vec<String>,
}

impl DockerCommand {
    fn new(args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

/// Build `docker run` for the managed persistent backend.
#[must_use]
pub fn provision(spec: &BackendSpec) -> DockerCommand {
    DockerCommand::new([
        "run",
        "-d",
        "--name",
        &spec.container,
        "--label",
        "io.helixir.managed=true",
        "-p",
        &format!("{}:{}", spec.port, spec.port),
        "-v",
        &format!("{}:/data", spec.volume),
        "-e",
        &format!("HELIX_PORT={}", spec.port),
        "-e",
        "HELIX_DATA_DIR=/data",
        "--restart",
        "unless-stopped",
        &spec.image,
    ])
}

/// Start an existing backend container.
#[must_use]
pub fn start(spec: &BackendSpec) -> DockerCommand {
    DockerCommand::new(["start", &spec.container])
}

/// Stop a backend before snapshotting or rollback.
#[must_use]
pub fn stop(spec: &BackendSpec) -> DockerCommand {
    DockerCommand::new(["stop", &spec.container])
}

/// Remove only the known managed container before recreating it on the same volume.
#[must_use]
pub fn remove(spec: &BackendSpec) -> DockerCommand {
    DockerCommand::new(["rm", &spec.container])
}

/// Create a tar snapshot of the persistent volume.  The caller chooses a
/// dedicated backup directory; it is never interpolated into a shell string.
#[must_use]
pub fn backup(spec: &BackendSpec, backup_dir: &Path, archive_name: &str) -> DockerCommand {
    DockerCommand::new([
        "run",
        "--rm",
        "-v",
        &format!("{}:/data:ro", spec.volume),
        "-v",
        &format!("{}:/out", backup_dir.display()),
        "alpine",
        "tar",
        "czf",
        &format!("/out/{archive_name}"),
        "-C",
        "/data",
        ".",
    ])
}

/// Validate the distributable HelixDB project with the pinned CLI.
#[must_use]
pub fn check_schema() -> Vec<String> {
    vec!["check".to_string(), "dev".to_string()]
}

/// Compile schema and queries into the managed local image.
#[must_use]
pub fn build_image() -> Vec<String> {
    vec![
        "build".to_string(),
        "--instance".to_string(),
        "dev".to_string(),
    ]
}

/// Content fingerprint persisted in the install manifest for idempotent reuse.
pub fn schema_fingerprint(schema_dir: &Path) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    for name in ["schema.hx", "queries.hx"] {
        hasher.update(name.as_bytes());
        hasher.update([0]);
        hasher.update(std::fs::read(schema_dir.join(name))?);
        hasher.update([0]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

/// Restore a snapshot into a volume after stopping the backend.
#[must_use]
pub fn restore(spec: &BackendSpec, backup_dir: &Path, archive_name: &str) -> DockerCommand {
    DockerCommand::new([
        "run",
        "--rm",
        "-v",
        &format!("{}:/data", spec.volume),
        "-v",
        &format!("{}:/out:ro", backup_dir.display()),
        "alpine",
        "tar",
        "xzf",
        &format!("/out/{archive_name}"),
        "-C",
        "/data",
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_and_restore_are_explicit_volume_operations() {
        let spec = BackendSpec::default();
        let backup = backup(&spec, Path::new("/tmp/helixir-backups"), "snapshot.tar.gz");
        assert_eq!(backup.args[0], "run");
        assert!(backup.args.iter().any(|arg| arg == "helixdb_data:/data:ro"));
        let restore = restore(&spec, Path::new("/tmp/helixir-backups"), "snapshot.tar.gz");
        assert!(restore.args.iter().any(|arg| arg == "helixdb_data:/data"));
        assert!(restore.args.iter().any(|arg| arg == "/out/snapshot.tar.gz"));
    }

    #[test]
    fn provision_has_restart_policy_and_persistent_data() {
        let args = provision(&BackendSpec::default()).args;
        assert!(
            args.windows(2)
                .any(|w| w == ["--restart", "unless-stopped"])
        );
        assert!(args.iter().any(|arg| arg == "helixdb_data:/data"));
    }
}
