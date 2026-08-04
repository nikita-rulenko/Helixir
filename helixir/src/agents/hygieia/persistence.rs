//! Persistent-storage safety checks.

use super::*;

impl Hygieia<'_> {
    /// The in-memory-storage trap (upstream HelixDB defaults newer builds to
    /// ephemeral storage; a stop ERASES everything unless started with the
    /// disk flag). Detector: the database serves data while the configured
    /// data dir holds no LMDB file — the corpus lives only in RAM and will
    /// die with the next restart. Loudest alert we have.
    pub async fn check_storage_persistence(&mut self) {
        let src = self.cfg().backup_source_dir.clone();
        if src.is_empty() {
            return;
        }
        let has_mdb = std::fs::read_dir(&src)
            .map(|rd| {
                rd.flatten().any(|e| {
                    let n = e.file_name().to_string_lossy().to_lowercase();
                    n.ends_with(".mdb") || n == "data.mdb" || n == "user"
                })
            })
            .unwrap_or(false);
        if has_mdb {
            return;
        }
        // Only alarm when the DB actually answers — an empty dir next to a
        // dead DB is a different (db_down) finding.
        let alive = self
            .tooling
            .db
            .execute_query::<serde_json::Value, _>(
                "getAllCategories",
                &serde_json::json!({"limit": 1}),
            )
            .await
            .is_ok();
        if alive {
            self.alert(
                "storage_not_persistent",
                &format!(
                    "database is SERVING but {src} holds no LMDB files — it may be running IN-MEMORY (newer HelixDB default); a restart will ERASE the corpus. Start it with disk persistence NOW."
                ),
                serde_json::Value::Null,
            )
            .await;
        }
    }
}
