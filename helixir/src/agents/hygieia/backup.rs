//! Backup age inspection and retention.

use super::*;

// ── Autobackup duty (#65) ────────────────────────────────────────────────────

/// Newest archive's age in hours, or None if the dir has no archives.
pub fn newest_backup_age_hours(dir: &std::path::Path) -> Option<f64> {
    let newest = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("helixir-data-"))
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()?;
    Some(newest.elapsed().ok()?.as_secs_f64() / 3600.0)
}

/// Keep the newest `keep` archives, delete the rest. Returns pruned count.
pub fn prune_backups(dir: &std::path::Path, keep: usize) -> usize {
    let mut archives: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("helixir-data-"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    archives.sort_by_key(|archive| std::cmp::Reverse(archive.0)); // newest first
    let mut pruned = 0;
    for (_, path) in archives.into_iter().skip(keep) {
        if std::fs::remove_file(&path).is_ok() {
            pruned += 1;
        }
    }
    pruned
}

impl Hygieia<'_> {
    /// The backup duty: when the newest archive is older than the configured
    /// interval, tar the data dir into `backup_dir`. With a known container
    /// the copy happens under `docker pause` — no writes land mid-copy, so
    /// the LMDB snapshot is consistent; the pause lasts only as long as the
    /// tar (a 32 MB corpus is sub-second). Journal on success, alert on
    /// failure. No-op when `backup_source_dir` is empty.
    pub async fn run_backup_duty(&mut self) {
        let cfg = self.cfg().clone();
        if cfg.backup_source_dir.is_empty() {
            return;
        }
        let backup_dir = if cfg.backup_dir.is_empty() {
            journal_path()
                .parent()
                .map(|p| p.join("backups"))
                .unwrap_or_else(|| PathBuf::from("./helixir-backups"))
        } else {
            PathBuf::from(&cfg.backup_dir)
        };
        if let Err(e) = std::fs::create_dir_all(&backup_dir) {
            self.alert(
                "backup_failed",
                &format!("cannot create backup dir {}: {e}", backup_dir.display()),
                serde_json::Value::Null,
            )
            .await;
            return;
        }
        if let Some(age) = newest_backup_age_hours(&backup_dir)
            && age < cfg.backup_interval_hours
        {
            return; // fresh enough
        }

        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let archive = backup_dir.join(format!("helixir-data-{stamp}.tar.gz"));

        let paused = if !cfg.container_name.is_empty() {
            tokio::process::Command::new("docker")
                .args(["pause", &cfg.container_name])
                .output()
                .await
                .map(|o| o.status.success())
                .unwrap_or(false)
        } else {
            false
        };

        let tar_ok = tokio::process::Command::new("tar")
            .args([
                "-czf",
                &archive.to_string_lossy(),
                "-C",
                &cfg.backup_source_dir,
                ".",
            ])
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);

        if paused {
            let unpaused = tokio::process::Command::new("docker")
                .args(["unpause", &cfg.container_name])
                .output()
                .await
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !unpaused {
                // A paused database is an outage — this must be LOUD.
                self.alert(
                    "backup_unpause_failed",
                    &format!(
                        "container {} is still PAUSED after backup — run `docker unpause {}` NOW",
                        cfg.container_name, cfg.container_name
                    ),
                    serde_json::Value::Null,
                )
                .await;
            }
        }

        if tar_ok {
            let size_mib = std::fs::metadata(&archive)
                .map(|m| m.len() as f64 / (1024.0 * 1024.0))
                .unwrap_or(0.0);
            let pruned = prune_backups(&backup_dir, cfg.backup_keep.max(1));
            journal(&HealthEvent {
                at: chrono::Utc::now().to_rfc3339(),
                severity: "heal".into(),
                kind: "backup_done".into(),
                summary: format!(
                    "{} written ({size_mib:.1} MiB, {} pruned, pause={})",
                    archive.display(),
                    pruned,
                    paused
                ),
                detail: serde_json::Value::Null,
            });
            info!("hygieia: backup done — {}", archive.display());
        } else {
            let _ = std::fs::remove_file(&archive);
            self.alert(
                "backup_failed",
                &format!(
                    "tar of {} failed — the corpus has NO fresh backup",
                    cfg.backup_source_dir
                ),
                serde_json::Value::Null,
            )
            .await;
        }
    }
}
