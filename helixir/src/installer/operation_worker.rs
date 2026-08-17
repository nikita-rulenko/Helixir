//! Typed child-process protocol for privileged installer execution.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use anyhow::{Context, ensure};

use super::{InstallEvent, InstallObserver, InstallOptions, InstallReport};

pub const EVENT_PREFIX: &str = "HELIXIR_INSTALL_EVENT=";
pub const REPORT_PREFIX: &str = "HELIXIR_INSTALL_REPORT=";

/// Run the hidden installer worker and forward its redaction-safe typed events.
pub fn run(
    options: &InstallOptions,
    observer: &dyn InstallObserver,
) -> anyhow::Result<InstallReport> {
    let binary = std::env::current_exe().context("resolve Helixir supervisor executable")?;
    let mut child = Command::new(binary)
        .arg("apply-install-json")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("start typed installer worker")?;
    let encoded = serde_json::to_vec(options).context("encode typed install options")?;
    child
        .stdin
        .take()
        .context("installer worker stdin unavailable")?
        .write_all(&encoded)
        .context("send typed install options")?;

    let stderr = child
        .stderr
        .take()
        .context("installer worker stderr unavailable")?;
    let stderr_reader = std::thread::spawn(move || std::io::read_to_string(stderr));
    let stdout = child
        .stdout
        .take()
        .context("installer worker stdout unavailable")?;
    let mut report: Option<InstallReport> = None;
    for line in BufReader::new(stdout).lines() {
        let line = line.context("read installer worker output")?;
        if let Some(encoded) = line.strip_prefix(EVENT_PREFIX) {
            let event: InstallEvent =
                serde_json::from_str(encoded).context("decode install event")?;
            observer.observe(event);
        } else if let Some(encoded) = line.strip_prefix(REPORT_PREFIX) {
            report = Some(serde_json::from_str(encoded).context("decode installer report")?);
        }
    }
    let status = child.wait().context("wait for installer worker")?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| anyhow::anyhow!("installer stderr reader panicked"))??;
    let report = report.context("installer worker returned no machine-readable report")?;
    ensure!(
        status.success() || !report.ready,
        "installer worker exited with {status}: {}",
        stderr.trim()
    );
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_prefixes_are_unambiguous() {
        assert_ne!(EVENT_PREFIX, REPORT_PREFIX);
        assert!(EVENT_PREFIX.ends_with('='));
        assert!(REPORT_PREFIX.ends_with('='));
    }
}
