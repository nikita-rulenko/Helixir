//! Shared helpers for the memory MCP routers.

/// Cached machine hostname for swarm presence — resolved once per process.
pub(super) fn machine_hostname() -> &'static str {
    use std::sync::OnceLock;

    static HOST: OnceLock<String> = OnceLock::new();
    HOST.get_or_init(|| {
        std::process::Command::new("hostname")
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|hostname| hostname.trim().to_string())
            .filter(|hostname| !hostname.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    })
}
