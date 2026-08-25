//! Opt-in, value-free HQL request tracing for isolated profiling runs.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

static TRACE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Serialize)]
struct QueryTraceRow<'a> {
    at: String,
    query: &'a str,
    parameter_keys: Vec<String>,
    attempt: u32,
    status: &'a str,
    duration_micros: u128,
    error_sha256: Option<String>,
}

pub(super) fn record<P: Serialize>(
    query: &str,
    params: &P,
    attempt: u32,
    status: &str,
    duration: Duration,
    error: Option<&str>,
) {
    let Some(path) = trace_path() else {
        return;
    };
    let _guard = TRACE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let row = QueryTraceRow {
        at: chrono::Utc::now().to_rfc3339(),
        query,
        parameter_keys: parameter_keys(params),
        attempt,
        status,
        duration_micros: duration.as_micros(),
        error_sha256: error.map(hash_text),
    };
    if let Err(error) = append_row(&path, &row) {
        eprintln!(
            "helixir: query trace write failed for {}: {error}",
            path.display()
        );
    }
}

fn trace_path() -> Option<PathBuf> {
    std::env::var_os("HELIXIR_QUERY_TRACE_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn parameter_keys<P: Serialize>(params: &P) -> Vec<String> {
    let Ok(serde_json::Value::Object(object)) = serde_json::to_value(params) else {
        return Vec::new();
    };
    let mut keys: Vec<_> = object.into_iter().map(|(key, _)| key).collect();
    keys.sort();
    keys
}

fn hash_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn append_row(path: &PathBuf, row: &QueryTraceRow<'_>) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path)?;
    serde_json::to_writer(&mut file, row).map_err(std::io::Error::other)?;
    file.write_all(b"\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn trace_contains_keys_and_hashes_but_never_values() {
        let directory = tempfile::tempdir().expect("private directory");
        #[cfg(unix)]
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))
            .expect("private permissions");
        let path = directory.path().join("query-trace.jsonl");
        temp_env::with_var(
            "HELIXIR_QUERY_TRACE_PATH",
            Some(path.to_string_lossy().as_ref()),
            || {
                record(
                    "searchMemory",
                    &serde_json::json!({"query": "private text", "limit": 50}),
                    2,
                    "error",
                    Duration::from_millis(12),
                    Some("private backend error"),
                );
            },
        );
        let raw = fs::read_to_string(path).expect("trace");
        assert!(raw.contains("searchMemory"));
        assert!(raw.contains("parameter_keys"));
        assert!(raw.contains("limit"));
        assert!(raw.contains("query"));
        assert!(!raw.contains("private text"));
        assert!(!raw.contains("private backend error"));
    }
}
