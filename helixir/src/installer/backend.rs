//! Persistent HelixDB/Docker adapter.
//!
//! The command builders are intentionally pure.  Applying them is a separate
//! concern, which lets `--dry-run`, tests, and a native UI share the exact same
//! backup/rollback contract.

use std::path::{Path, PathBuf};

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

/// Deploy the bundled schema through the installed deploy helper.
#[must_use]
pub fn deploy_schema(deploy_binary: &Path, spec: &BackendSpec) -> Vec<String> {
    vec![
        deploy_binary.display().to_string(),
        "--host".to_string(),
        spec.host.clone(),
        "--port".to_string(),
        spec.port.to_string(),
        "--schema-dir".to_string(),
        spec.schema_dir.display().to_string(),
    ]
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
