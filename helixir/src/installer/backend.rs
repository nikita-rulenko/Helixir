//! Persistent HelixDB/Docker adapter.
//!
//! The command builders are intentionally pure.  Applying them is a separate
//! concern, which lets `--dry-run`, tests, and a native UI share the exact same
//! backup/rollback contract.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// HelixDB CLI version compatible with this LMDB-era schema.
pub const HELIX_CLI_VERSION: &str = "2.3.5";
/// Exact upstream source snapshot maintained by this repository.
pub const UPSTREAM_REVISION: &str = "17e7ecf764aecd553e1f54ca25320d654153a9aa";
/// Helixir-maintained patch level layered on the pinned upstream engine.
/// A plain upstream v2.3.5 binary is not equivalent because it lacks the
/// indexed collection and bounded-reader fixes required by the memory gate.
pub const ENGINE_REVISION: &str = "helixir-v2.3.5-indexed-v1";
/// Release-only descriptor placed beside the packaged schema.
pub const BACKEND_IMAGE_DESCRIPTOR: &str = "backend-image.json";
/// Read-only contract returned by the deployed query inventory.
pub const SCHEMA_CONTRACT_VERSION: &str = "helixir-rbac-moirai-v4";
/// Local HelixDB is intentionally bounded: the upstream gateway creates eight
/// workers per visible core and each worker can retain a request high-water
/// mark in its mimalloc heap.
pub const MANAGED_HELIX_CORES: &str = "1";
/// Each core gets two read workers; the upstream default of eight multiplied
/// independent per-query arenas and was a direct contributor to RSS spikes.
pub const MANAGED_HELIX_WORKERS_PER_CORE: &str = "2";
/// Immediate decommit reduces allocator-retained free pages after requests;
/// graph scans can still retain live arena-backed material upstream.
pub const MIMALLOC_PURGE_DELAY: &str = "0";
/// Purges must use `MADV_DONTNEED`, not lazy `MADV_FREE`, so RSS falls now.
pub const MIMALLOC_PURGE_DECOMMITS: &str = "1";
/// Do not multiply the immediate purge delay for mimalloc arenas.
pub const MIMALLOC_ARENA_PURGE_MULT: &str = "1";
/// Hard backstop for the managed local backend.
pub const MANAGED_MEMORY_LIMIT: &str = "3g";
/// Docker's byte representation of [`MANAGED_MEMORY_LIMIT`].
pub const MANAGED_MEMORY_LIMIT_BYTES: i64 = 3 * 1024 * 1024 * 1024;

/// Immutable managed-backend payload selected by a server release.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackendImageDescriptor {
    pub format_version: u32,
    pub image: String,
    pub engine_revision: String,
    pub schema_fingerprint: String,
    pub source_url: String,
    pub source_sha256: String,
    pub upstream_revision: String,
    pub fork_revision: String,
    pub license: String,
}

impl BackendImageDescriptor {
    /// Load and validate a packaged descriptor. Source-tree installs may omit
    /// it and compile with the checked-in fork instead.
    pub fn load(project_dir: &Path) -> anyhow::Result<Option<Self>> {
        let path = project_dir.join(BACKEND_IMAGE_DESCRIPTOR);
        if !path.is_file() {
            return Ok(None);
        }
        let descriptor: Self = serde_json::from_slice(&std::fs::read(&path)?)?;
        anyhow::ensure!(
            descriptor.format_version == 1,
            "unsupported descriptor format"
        );
        anyhow::ensure!(
            digest_pinned_image(&descriptor.image),
            "managed HelixDB image must be pinned by an exact sha256 digest"
        );
        anyhow::ensure!(
            descriptor.engine_revision == ENGINE_REVISION,
            "managed HelixDB descriptor engine revision mismatch"
        );
        let expected = schema_fingerprint(&project_dir.join("schema"))?;
        anyhow::ensure!(
            descriptor.schema_fingerprint == expected,
            "managed HelixDB descriptor schema fingerprint mismatch"
        );
        anyhow::ensure!(
            descriptor.source_url.starts_with("https://"),
            "managed HelixDB source URL must use HTTPS"
        );
        anyhow::ensure!(
            is_hex_digest(&descriptor.source_sha256, 64),
            "managed HelixDB source checksum is invalid"
        );
        anyhow::ensure!(
            descriptor.upstream_revision == UPSTREAM_REVISION,
            "managed HelixDB upstream revision mismatch"
        );
        anyhow::ensure!(
            is_hex_digest(&descriptor.fork_revision, 40),
            "managed HelixDB fork revision is invalid"
        );
        anyhow::ensure!(
            descriptor.license == "AGPL-3.0-only",
            "managed HelixDB source license metadata is invalid"
        );
        Ok(Some(descriptor))
    }
}

fn digest_pinned_image(image: &str) -> bool {
    let Some((repository, digest)) = image.split_once("@sha256:") else {
        return false;
    };
    !repository.is_empty() && !repository.contains('@') && is_hex_digest(digest, 64)
}

fn is_hex_digest(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Managed backend specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendSpec {
    /// Hostname/address used for health and schema deployment.
    pub host: String,
    /// Docker container name.
    pub container: String,
    /// Local image produced by HelixDB v2.3.5.
    pub image: String,
    /// Maintained engine patch level, independent of the upstream CLI version.
    pub engine_revision: String,
    /// Exact schema/query contract compiled into the image.
    pub schema_fingerprint: String,
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
            engine_revision: ENGINE_REVISION.to_string(),
            schema_fingerprint: String::new(),
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
        "--label",
        &format!("io.helixir.engine-revision={}", spec.engine_revision),
        "--label",
        &format!("io.helixir.schema-fingerprint={}", spec.schema_fingerprint),
        "--memory",
        MANAGED_MEMORY_LIMIT,
        "--memory-swap",
        MANAGED_MEMORY_LIMIT,
        "-p",
        &format!("{}:{}", spec.port, spec.port),
        "-v",
        &format!("{}:/data", spec.volume),
        "-e",
        &format!("HELIX_PORT={}", spec.port),
        "-e",
        "HELIX_DATA_DIR=/data",
        "-e",
        &format!("HELIX_CORES_OVERRIDE={MANAGED_HELIX_CORES}"),
        "-e",
        &format!("HELIX_WORKERS_PER_CORE={MANAGED_HELIX_WORKERS_PER_CORE}"),
        "-e",
        &format!("MIMALLOC_PURGE_DELAY={MIMALLOC_PURGE_DELAY}"),
        "-e",
        &format!("MIMALLOC_PURGE_DECOMMITS={MIMALLOC_PURGE_DECOMMITS}"),
        "-e",
        &format!("MIMALLOC_ARENA_PURGE_MULT={MIMALLOC_ARENA_PURGE_MULT}"),
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

/// Remove the current contents of a stopped managed volume before restore.
#[must_use]
pub fn clear_volume(spec: &BackendSpec) -> DockerCommand {
    DockerCommand::new([
        "run",
        "--rm",
        "-v",
        &format!("{}:/data", spec.volume),
        "alpine",
        "find",
        "/data",
        "-mindepth",
        "1",
        "-delete",
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
        let clear = clear_volume(&spec);
        assert!(clear.args.windows(2).any(|args| args == ["-mindepth", "1"]));
        assert!(clear.args.iter().any(|arg| arg == "-delete"));
    }

    #[test]
    fn provision_has_restart_policy_and_persistent_data() {
        let spec = BackendSpec {
            schema_fingerprint: "sha256:test".to_string(),
            ..BackendSpec::default()
        };
        let args = provision(&spec).args;
        assert!(
            args.windows(2)
                .any(|w| w == ["--restart", "unless-stopped"])
        );
        assert!(args.iter().any(|arg| arg == "helixdb_data:/data"));
        assert!(
            args.iter()
                .any(|arg| { arg == &format!("io.helixir.engine-revision={ENGINE_REVISION}") })
        );
        assert!(
            args.iter()
                .any(|arg| arg == "io.helixir.schema-fingerprint=sha256:test")
        );
        for expected in [
            "HELIX_CORES_OVERRIDE=1",
            "HELIX_WORKERS_PER_CORE=2",
            "MIMALLOC_PURGE_DELAY=0",
            "MIMALLOC_PURGE_DECOMMITS=1",
            "MIMALLOC_ARENA_PURGE_MULT=1",
        ] {
            assert!(args.iter().any(|arg| arg == expected));
        }
        assert!(
            args.windows(2)
                .any(|window| window == ["--memory", MANAGED_MEMORY_LIMIT])
        );
        assert!(
            args.windows(2)
                .any(|window| window == ["--memory-swap", MANAGED_MEMORY_LIMIT])
        );
    }

    #[test]
    fn packaged_descriptor_is_digest_and_contract_pinned() {
        let root = std::env::temp_dir().join(format!(
            "helixir-backend-descriptor-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(root.join("schema")).unwrap();
        std::fs::write(root.join("schema/schema.hx"), "N::Memory { id: String }").unwrap();
        std::fs::write(
            root.join("schema/queries.hx"),
            "QUERY health() => RETURN \"ok\"",
        )
        .unwrap();
        let fingerprint = schema_fingerprint(&root.join("schema")).unwrap();
        let descriptor = BackendImageDescriptor {
            format_version: 1,
            image: format!("ghcr.io/example/helixir-helixdb@sha256:{}", "a".repeat(64)),
            engine_revision: ENGINE_REVISION.to_string(),
            schema_fingerprint: fingerprint,
            source_url: "https://example.test/source.tar.gz".to_string(),
            source_sha256: "b".repeat(64),
            upstream_revision: UPSTREAM_REVISION.to_string(),
            fork_revision: "c".repeat(40),
            license: "AGPL-3.0-only".to_string(),
        };
        std::fs::write(
            root.join(BACKEND_IMAGE_DESCRIPTOR),
            serde_json::to_vec(&descriptor).unwrap(),
        )
        .unwrap();
        assert_eq!(
            BackendImageDescriptor::load(&root).unwrap(),
            Some(descriptor)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn descriptor_rejects_tag_only_and_malformed_digests() {
        assert!(!digest_pinned_image(
            "ghcr.io/example/helixir-helixdb:latest"
        ));
        assert!(!digest_pinned_image(
            "ghcr.io/example/helixir-helixdb@sha256:abcd"
        ));
        assert!(digest_pinned_image(&format!(
            "ghcr.io/example/helixir-helixdb@sha256:{}",
            "d".repeat(64)
        )));
    }
}
