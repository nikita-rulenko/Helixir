//! Managed HelixDB backup inventory, verification and guarded restore.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

use super::backend::{self, BackendSpec, DockerCommand};

/// One archive in the operator-controlled backup vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRecord {
    pub id: String,
    pub created_at: String,
    pub size_bytes: u64,
    pub kind: String,
}

/// Current managed-backend vault state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInventory {
    pub available: bool,
    pub reason: Option<String>,
    pub directory: String,
    pub retention: usize,
    pub archives: Vec<BackupRecord>,
}

/// Receipt for snapshot, verification, or restore mutations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupReceipt {
    pub operation: String,
    pub backup_id: String,
    pub safety_backup_id: Option<String>,
    pub message: String,
}

/// Restore request guarded by an exact human-readable confirmation phrase.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreRequest {
    pub backup_id: String,
    pub confirmation: String,
}

/// List bounded archives for the managed backend without accepting a path.
pub fn inventory() -> BackupInventory {
    let directory = backup_dir();
    let retention = crate::core::HelixirConfig::from_env()
        .watchdog
        .backup_keep
        .max(1);
    match managed_spec() {
        Ok(_) => BackupInventory {
            available: true,
            reason: None,
            directory: display_path(&directory),
            retention,
            archives: list_archives(&directory, true),
        },
        Err(error) => BackupInventory {
            available: false,
            reason: Some(error.to_string()),
            directory: display_path(&directory),
            retention,
            archives: list_archives(&directory, true),
        },
    }
}

/// Create a cold, consistent snapshot and restart the known managed container.
pub fn create() -> anyhow::Result<BackupReceipt> {
    let id = format!(
        "helixdb-manual-{}.tar.gz",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    create_named(&id)?;
    prune_inventory()?;
    remember_latest(&id)?;
    Ok(BackupReceipt {
        operation: "backup_created".to_string(),
        backup_id: id.clone(),
        safety_backup_id: None,
        message: format!("consistent managed-volume snapshot {id} is ready"),
    })
}

/// Verify that an inventory archive is a readable gzip tar without extracting it.
pub fn verify(backup_id: &str) -> anyhow::Result<BackupReceipt> {
    let path = resolve_archive(backup_id)?;
    let status = Command::new("tar")
        .args(["-tzf"])
        .arg(&path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("verify backup archive")?;
    ensure!(
        status.success(),
        "backup archive failed integrity verification"
    );
    Ok(BackupReceipt {
        operation: "backup_verified".to_string(),
        backup_id: backup_id.to_string(),
        safety_backup_id: None,
        message: "archive is readable and remains inside the managed vault".to_string(),
    })
}

/// Restore one verified inventory archive after taking a fresh safety snapshot.
pub fn restore(request: &RestoreRequest) -> anyhow::Result<BackupReceipt> {
    ensure!(
        request.confirmation == format!("RESTORE {}", request.backup_id),
        "restore confirmation must exactly match RESTORE <backup-id>"
    );
    verify(&request.backup_id)?;
    let safety_id = format!(
        "helixdb-pre-restore-{}.tar.gz",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    create_named(&safety_id)?;
    let spec = managed_spec()?;
    let directory = backup_dir();
    let restored = restore_archive(&spec, &directory, &request.backup_id);
    if let Err(error) = restored {
        let rollback = restore_archive(&spec, &directory, &safety_id);
        return Err(match rollback {
            Ok(()) => error.context("restore failed; safety snapshot was restored"),
            Err(rollback_error) => anyhow::anyhow!(
                "restore failed: {error}; safety rollback also failed: {rollback_error}"
            ),
        });
    }
    Ok(BackupReceipt {
        operation: "backup_restored".to_string(),
        backup_id: request.backup_id.clone(),
        safety_backup_id: Some(safety_id.clone()),
        message: format!(
            "{} restored; rollback snapshot {} was preserved",
            request.backup_id, safety_id
        ),
    })
}

/// Restore and prove that the resulting database exposes this build's schema contract.
pub async fn restore_verified(request: RestoreRequest) -> anyhow::Result<BackupReceipt> {
    let receipt = tokio::task::spawn_blocking(move || restore(&request))
        .await
        .context("join backup restore worker")??;
    let spec = managed_spec()?;
    if super::native::probe_backend_schema_contract(&spec.host, spec.port).await {
        return Ok(receipt);
    }
    let safety_id = receipt
        .safety_backup_id
        .as_deref()
        .context("restore produced no safety snapshot")?
        .to_string();
    let rollback_spec = spec.clone();
    let rollback_id = safety_id.clone();
    tokio::task::spawn_blocking(move || {
        restore_archive(&rollback_spec, &backup_dir(), &rollback_id)
    })
    .await
    .context("join incompatible-restore rollback worker")??;
    ensure!(
        super::native::probe_backend_schema_contract(&spec.host, spec.port).await,
        "restored archive was schema-incompatible and the safety rollback did not recover the current schema"
    );
    anyhow::bail!(
        "restored archive was incompatible with the current Helixir schema; safety snapshot {safety_id} was restored"
    )
}

fn create_named(archive_name: &str) -> anyhow::Result<()> {
    validate_id(archive_name)?;
    let spec = managed_spec()?;
    let directory = backup_dir();
    std::fs::create_dir_all(&directory)?;
    run(backend::stop(&spec)).context("stop managed backend for snapshot")?;
    let backup = run(backend::backup(&spec, &directory, archive_name));
    let restart = run(backend::start(&spec));
    match (backup, restart) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error).context("snapshot failed; backend restarted"),
        (Ok(()), Err(error)) => Err(error).context("snapshot succeeded but backend restart failed"),
        (Err(error), Err(restart_error)) => {
            anyhow::bail!("snapshot failed: {error}; backend restart also failed: {restart_error}")
        }
    }
}

fn restore_archive(spec: &BackendSpec, directory: &Path, backup_id: &str) -> anyhow::Result<()> {
    run_allow_missing(
        backend::stop(spec),
        &["is not running", "No such container"],
    )?;
    run_allow_missing(backend::remove(spec), &["No such container"])?;
    run(backend::clear_volume(spec)).context("clear managed volume before restore")?;
    run(backend::restore(spec, directory, backup_id))
        .context("extract backup into managed volume")?;
    run(backend::provision(spec)).context("restart managed backend after restore")?;
    wait_until_reachable(spec)
}

fn managed_spec() -> anyhow::Result<BackendSpec> {
    let manifest =
        super::manifest::read(&manifest_path())?.context("installation manifest is missing")?;
    ensure!(
        manifest.backend.kind == "managed_local",
        "backup administration requires a managed local HelixDB"
    );
    ensure!(
        !manifest.backend.volume.is_empty(),
        "managed backend volume is missing from the manifest"
    );
    Ok(BackendSpec {
        host: manifest.backend.host,
        port: manifest.backend.port,
        container: manifest.backend.container,
        volume: manifest.backend.volume,
        image: manifest.backend.image,
        ..BackendSpec::default()
    })
}

fn list_archives(directory: &Path, bounded: bool) -> Vec<BackupRecord> {
    let mut archives: Vec<_> = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let id = entry.file_name().to_string_lossy().to_string();
            if validate_id(&id).is_err() {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            let created = metadata.modified().ok()?;
            Some(BackupRecord {
                kind: if id.contains("pre-restore") {
                    "safety"
                } else if id.contains("manual") {
                    "manual"
                } else {
                    "automatic"
                }
                .to_string(),
                id,
                created_at: chrono::DateTime::<chrono::Utc>::from(created).to_rfc3339(),
                size_bytes: metadata.len(),
            })
        })
        .collect();
    archives.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    if bounded {
        archives.truncate(100);
    }
    archives
}

fn prune_inventory() -> anyhow::Result<()> {
    let directory = backup_dir();
    let keep = crate::core::HelixirConfig::from_env()
        .watchdog
        .backup_keep
        .max(1);
    for archive in list_archives(&directory, false).into_iter().skip(keep) {
        std::fs::remove_file(resolve_archive(&archive.id)?)?;
    }
    Ok(())
}

fn wait_until_reachable(spec: &BackendSpec) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(45);
    let address = format!("{}:{}", spec.host, spec.port)
        .parse()
        .context("parse restored backend address")?;
    while Instant::now() < deadline {
        if std::net::TcpStream::connect_timeout(&address, Duration::from_secs(1)).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    anyhow::bail!("restored backend did not become reachable within 45 seconds")
}

fn display_path(path: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(home()) {
        return format!("~/{}", relative.display());
    }
    path.display().to_string()
}

fn remember_latest(id: &str) -> anyhow::Result<()> {
    let path = manifest_path();
    let Some(mut manifest) = super::manifest::read(&path)? else {
        return Ok(());
    };
    manifest.last_backup = Some(backup_dir().join(id));
    super::manifest::write(&path, &manifest)?;
    Ok(())
}

fn resolve_archive(id: &str) -> anyhow::Result<PathBuf> {
    validate_id(id)?;
    let path = backup_dir().join(id);
    ensure!(path.is_file(), "backup archive does not exist");
    Ok(path)
}

fn validate_id(id: &str) -> anyhow::Result<()> {
    ensure!(
        id.len() <= 180 && id.ends_with(".tar.gz"),
        "invalid backup id"
    );
    ensure!(
        id.starts_with("helixdb-") || id.starts_with("helixir-data-"),
        "archive is not managed by Helixir"
    );
    ensure!(
        id.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')),
        "backup id contains unsafe characters"
    );
    Ok(())
}

fn run(command: DockerCommand) -> anyhow::Result<()> {
    let status = Command::new("docker")
        .args(&command.args)
        .status()
        .context("run Docker backup operation")?;
    ensure!(status.success(), "docker operation exited with {status}");
    Ok(())
}

fn run_allow_missing(command: DockerCommand, allowed: &[&str]) -> anyhow::Result<()> {
    let output = Command::new("docker")
        .args(&command.args)
        .output()
        .context("run Docker restore operation")?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    ensure!(
        allowed.iter().any(|value| stderr.contains(value)),
        "docker operation failed: {}",
        stderr.trim()
    );
    Ok(())
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn backup_dir() -> PathBuf {
    let configured = crate::core::HelixirConfig::from_env().watchdog.backup_dir;
    if configured.is_empty() {
        home().join(".helixir/backups")
    } else {
        PathBuf::from(configured)
    }
}

fn manifest_path() -> PathBuf {
    home().join(".helixir/install.json")
}

#[cfg(test)]
#[path = "backups_tests.rs"]
mod tests;
