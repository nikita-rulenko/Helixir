//! Typed hot-reload coordination shared by CLI and the native supervisor.

use serde::{Deserialize, Serialize};

/// Bounded summary of reload signals and processes that need a restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReloadReceipt {
    pub signalled_processes: usize,
    pub failed_signals: usize,
    pub restart_required: Vec<String>,
}

/// Signal reload-capable processes and report deeper snapshots needing restart.
pub fn reload() -> anyhow::Result<ReloadReceipt> {
    #[cfg(unix)]
    {
        let output = std::process::Command::new("pgrep")
            .args(["-f", "helixir-mcp|helixir gateway"])
            .output()?;
        let pids: Vec<i32> = String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .filter_map(|value| value.parse().ok())
            .filter(|pid| *pid != std::process::id() as i32)
            .collect();
        let mut signalled_processes = 0;
        let mut failed_signals = 0;
        for pid in pids {
            if std::process::Command::new("kill")
                .args(["-HUP", &pid.to_string()])
                .status()
                .is_ok_and(|status| status.success())
            {
                signalled_processes += 1;
            } else {
                failed_signals += 1;
            }
        }
        let restart_required = ["daemon", "watch"]
            .into_iter()
            .filter(|name| process_is_alive(name))
            .map(str::to_string)
            .collect();
        Ok(ReloadReceipt {
            signalled_processes,
            failed_signals,
            restart_required,
        })
    }
    #[cfg(not(unix))]
    Ok(ReloadReceipt {
        signalled_processes: 0,
        failed_signals: 0,
        restart_required: vec!["all Helixir processes".to_string()],
    })
}

#[cfg(unix)]
fn process_is_alive(name: &str) -> bool {
    let path = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(format!(".helixir/{name}.pid"));
    let Ok(raw) = std::fs::read_to_string(path) else {
        return false;
    };
    let pid = serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|value| value.get("pid").and_then(serde_json::Value::as_i64))
        .unwrap_or(0);
    pid > 0
        && std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .is_ok_and(|status| status.success())
}
