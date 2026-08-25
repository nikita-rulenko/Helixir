//! Redacted JSONL tracing. Request and response bodies never cross this boundary.

use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

#[derive(Clone, Debug, Default)]
pub struct TraceSink {
    file: Option<Arc<Mutex<File>>>,
}

#[derive(Debug, Serialize)]
pub struct TraceEvent<'a> {
    pub timestamp_ms: u128,
    pub request_id: u64,
    pub query: &'a str,
    pub profile: &'a str,
    pub status: u16,
    pub latency_micros: u64,
    pub request_bytes: usize,
    pub response_bytes: usize,
    pub request_sha256: &'a str,
    pub response_sha256: &'a str,
    pub parameter_names: &'a [String],
    pub response_shape: &'a BTreeMap<String, String>,
    pub response_cardinality: &'a BTreeMap<String, usize>,
    pub state_records_before: usize,
    pub state_records_after: usize,
    pub process_rss_bytes: u64,
}

impl TraceSink {
    /// Open an optional hardened trace file.
    ///
    /// # Errors
    ///
    /// Returns an error for repository-local paths, invalid permissions, or
    /// filesystem failures.
    pub fn open(path: Option<&Path>) -> Result<Self> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let path = absolute_path(path)?;
        reject_repository_path(&path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create trace directory {}", parent.display()))?;
            secure_directory(parent)?;
        }
        let file = secure_file(&path)?;
        Ok(Self {
            file: Some(Arc::new(Mutex::new(File::from_std(file)))),
        })
    }

    /// Append one body-free event.
    ///
    /// # Errors
    ///
    /// Returns serialization and filesystem write failures.
    pub async fn record(&self, event: &TraceEvent<'_>) -> Result<()> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        let mut line = serde_json::to_vec(event)?;
        line.push(b'\n');
        let mut file = file.lock().await;
        file.write_all(&line).await?;
        file.flush().await?;
        Ok(())
    }
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    if let (Some(parent), Some(name)) = (absolute.parent(), absolute.file_name())
        && parent.exists()
    {
        return Ok(parent.canonicalize()?.join(name));
    }
    Ok(absolute)
}

fn reject_repository_path(path: &Path) -> Result<()> {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repository = crate_root.parent().unwrap_or(crate_root);
    if path.starts_with(repository) {
        bail!(
            "trace path must be outside the repository: {}",
            path.display()
        );
    }
    Ok(())
}

fn secure_file(path: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .with_context(|| format!("open trace {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn secure_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event<'a>(
        shape: &'a BTreeMap<String, String>,
        cardinality: &'a BTreeMap<String, usize>,
        parameter_names: &'a [String],
    ) -> TraceEvent<'a> {
        TraceEvent {
            timestamp_ms: 1,
            request_id: 2,
            query: "addMemory",
            profile: "fast",
            status: 200,
            latency_micros: 3,
            request_bytes: 4,
            response_bytes: 5,
            request_sha256: "request-hash",
            response_sha256: "response-hash",
            parameter_names,
            response_shape: shape,
            response_cardinality: cardinality,
            state_records_before: 1,
            state_records_after: 2,
            process_rss_bytes: 42,
        }
    }

    #[tokio::test]
    async fn trace_is_redacted_and_permission_hardened() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("private");
        let path = directory.join("trace.jsonl");
        let sink = TraceSink::open(Some(&path)).unwrap();
        let shape = BTreeMap::new();
        let cardinality = BTreeMap::new();
        let parameter_names = ["content".to_owned(), "api_key".to_owned()];
        sink.record(&event(&shape, &cardinality, &parameter_names))
            .await
            .unwrap();
        let text = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(text.contains("request-hash"));
        assert!(text.contains("response-hash"));
        assert!(!text.contains("secret memory"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(directory).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[tokio::test]
    async fn repository_trace_path_is_rejected() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("forbidden.jsonl");
        assert!(TraceSink::open(Some(&path)).is_err());
    }

    #[tokio::test]
    async fn sibling_repository_trace_path_is_rejected() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("forbidden.jsonl");
        assert!(TraceSink::open(Some(&path)).is_err());
    }
}
