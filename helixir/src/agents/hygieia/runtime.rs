//! Runtime policies and process-level health inspection.

use super::*;

/// Pure orphan policy (unit-testable): a fresh daemon heartbeat + every
/// non-daemon agent silent past the horizon → that daemon's id.
pub fn orphan_daemon(
    roster: &[crate::toolkit::tooling_manager::swarm::AgentPresence],
    now: chrono::DateTime<chrono::Utc>,
    horizon_secs: i64,
) -> Option<String> {
    let daemon = roster
        .iter()
        .find(|a| a.role == "daemon" && a.is_active(now, 1800))?;
    let others_active = roster
        .iter()
        .any(|a| a.role != "daemon" && a.is_active(now, horizon_secs));
    if others_active {
        None
    } else {
        Some(daemon.agent_id.clone())
    }
}

/// `docker stats` one-shot for a container's memory cell.
pub async fn sample_container_memory(name: &str) -> Option<MemSample> {
    let out = tokio::process::Command::new("docker")
        .args(["stats", "--no-stream", "--format", "{{.MemUsage}}", name])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_mem_usage(String::from_utf8_lossy(&out.stdout).trim())
}

pub(super) async fn restart_container(name: &str) -> bool {
    tokio::process::Command::new("docker")
        .args(["restart", name])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// #89: ask the kernel to shed reclaimable memory (page cache) charged to
/// the container's cgroup — targeted and safe: live heap is never touched.
/// cgroupfs is read-only inside the container, so the write goes through a
/// short-lived privileged helper in the host's cgroup namespace. Covers
/// both layouts: Docker Desktop (`/sys/fs/cgroup/docker/<id>`) and native
/// Linux (`system.slice/docker-<id>.scope`). The kernel returns EAGAIN when
/// it reclaims less than asked — the `|| true` keeps that a success; the
/// caller's re-sample is the judge of whether it helped.
pub(super) async fn reclaim_container_cache(name: &str, step_mib: u64) -> bool {
    let Ok(out) = tokio::process::Command::new("docker")
        .args(["inspect", "-f", "{{.Id}}", name])
        .output()
        .await
    else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    let script = format!(
        "for p in /sys/fs/cgroup/docker/{id} /sys/fs/cgroup/system.slice/docker-{id}.scope; do \
           if [ -f \"$p/memory.reclaim\" ]; then echo {step_mib}M > \"$p/memory.reclaim\" || true; exit 0; fi; \
         done; exit 1"
    );
    tokio::process::Command::new("docker")
        .args([
            "run",
            "--rm",
            "--privileged",
            "--pid=host",
            "--cgroupns=host",
            "alpine",
            "sh",
            "-c",
            &script,
        ])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}
