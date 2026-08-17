//! Persistent browser-session credentials for the local admin control plane.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};

const CONTAINER_TOKEN_PATH: &str = "/run/secrets/helixir-control-plane-token";

/// Resolve the browser-token path used by native and container runtimes.
#[must_use]
pub fn token_path(explicit: Option<&Path>, containerized: bool) -> PathBuf {
    explicit
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("HELIXIR_CONTROL_PLANE_TOKEN_FILE").map(PathBuf::from))
        .unwrap_or_else(|| {
            if containerized {
                PathBuf::from(CONTAINER_TOKEN_PATH)
            } else {
                default_token_path()
            }
        })
}

/// Load the configured browser token, creating it only for a native runtime.
pub fn load_token(path: &Path, containerized: bool) -> anyhow::Result<String> {
    if containerized {
        return read_token(path)?.with_context(|| {
            format!(
                "control-plane browser token is missing at {}; initialize and mount the secret before starting the container",
                path.display()
            )
        });
    }
    load_or_create_token(path)
}

/// Load the stable private browser token, creating it atomically on first use.
pub fn load_or_create_token(path: &Path) -> anyhow::Result<String> {
    if let Some(token) = read_token(path)? {
        protect_existing(path)?;
        return Ok(token);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create control-plane state dir {}", parent.display()))?;
    }
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    match private_create(path) {
        Ok(mut file) => {
            file.write_all(token.as_bytes())?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            Ok(token)
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            read_token(path)?.context("control-plane token appeared concurrently but is invalid")
        }
        Err(error) => {
            Err(error).with_context(|| format!("create control-plane token {}", path.display()))
        }
    }
}

/// Default private state-file location shared with the container secret mount.
#[must_use]
pub fn default_token_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".helixir/run/control-plane-browser.token")
}

fn read_token(path: &Path) -> anyhow::Result<Option<String>> {
    let value = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let token = value.trim().to_string();
    ensure!(
        token.len() >= 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "control-plane browser token file is invalid"
    );
    Ok(Some(token))
}

fn protect_existing(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect control-plane token {}", path.display()))?;
    ensure!(
        metadata.file_type().is_file(),
        "control-plane browser token must be a regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("protect control-plane token {}", path.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn private_create(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn private_create(path: &Path) -> std::io::Result<std::fs::File> {
    OpenOptions::new().create_new(true).write(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_path(label: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("helixir-{label}-{}", uuid::Uuid::new_v4().simple()))
            .join("token")
    }

    #[test]
    fn native_token_is_stable_high_entropy_and_private() {
        let path = temporary_path("browser-token");
        let first = load_token(&path, false).unwrap();
        let second = load_token(&path, false).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn container_mode_never_creates_a_missing_secret() {
        let path = temporary_path("missing-browser-token");
        let error = load_token(&path, true).unwrap_err();
        assert!(error.to_string().contains("initialize and mount"));
        assert!(!path.exists());
    }

    #[test]
    fn malformed_secret_fails_closed() {
        let path = temporary_path("malformed-browser-token");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not-a-valid-token\n").unwrap();
        let error = load_token(&path, true).unwrap_err();
        assert!(error.to_string().contains("invalid"));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn native_load_repairs_an_insecure_existing_token() {
        use std::os::unix::fs::PermissionsExt;

        let path = temporary_path("insecure-browser-token");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("{}\n", "c".repeat(64))).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        load_token(&path, false).unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
