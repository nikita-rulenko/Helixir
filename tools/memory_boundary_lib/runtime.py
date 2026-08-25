"""Docker isolation, sampling, guard enforcement and scenario execution."""

from __future__ import annotations

import json
import os
import signal
import subprocess
import time
from pathlib import Path
from typing import Any

from .contract import HarnessError, ROOT, docker_env, sha256_file
from .credentials import private_llm_env
from .profiling import hooks_for, start_profiler, stop_process, stop_profiler
from .reporting import collect_profiler_artifacts


def run_checked(
    command: list[str], *, env: dict[str, str], cwd: str | None = None
) -> str:
    result = subprocess.run(command, cwd=cwd, env=env, text=True, capture_output=True)
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise HarnessError(
            f"command failed ({result.returncode}): {command!r}: {detail}"
        )
    return result.stdout.strip()


def docker_inspect(container: str, env: dict[str, str]) -> dict[str, Any]:
    rows = json.loads(run_checked(["docker", "inspect", container], env=env))
    if len(rows) != 1:
        raise HarnessError(f"expected one container named {container}")
    return rows[0]


def preflight(trace: dict[str, Any]) -> None:
    if os.environ.get("HELIXIR_MEMORY_HARNESS_DISPOSABLE_DOCKER") != "1":
        raise HarnessError(
            "live run requires HELIXIR_MEMORY_HARNESS_DISPOSABLE_DOCKER=1"
        )
    env = docker_env(trace)
    daemon_id = run_checked(["docker", "info", "--format", "{{.ID}}"], env=env)
    if daemon_id != trace["disposable_daemon_id"]:
        raise HarnessError(
            f"Docker daemon mismatch: expected {trace['disposable_daemon_id']}, got {daemon_id}"
        )
    ports = run_checked(["docker", "ps", "-a", "--format", "{{.Ports}}"], env=env)
    if "6970->" in ports or "0.0.0.0:6970" in ports or "127.0.0.1:6970" in ports:
        raise HarnessError(
            "disposable daemon unexpectedly exposes production port 6970"
        )
    candidate = Path(trace["run"]["candidate_binary_path"]).expanduser().resolve()
    if sha256_file(candidate) != trace["run"]["candidate_binary_sha256"]:
        raise HarnessError("candidate binary SHA-256 mismatch")


def verify_cold_files(target: dict[str, Any]) -> list[dict[str, Any]]:
    records = []
    for row in target.get("cold_files", []):
        path = Path(row["path"]).expanduser().resolve()
        actual = sha256_file(path)
        if actual != row["sha256"]:
            raise HarnessError(f"cold-copy checksum mismatch for {path}: {actual}")
        records.append(
            {"path": str(path), "sha256": actual, "size": path.stat().st_size}
        )
    return records


def reset_target(
    target: dict[str, Any], env: dict[str, str], prior_started: str | None
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    run_checked(target["reset_command"], env=env)
    run_checked(target["ready_command"], env=env)
    inspect = docker_inspect(target["container"], env)
    state = inspect["State"]
    if (
        not state.get("Running")
        or state.get("OOMKilled")
        or int(inspect.get("RestartCount", 0)) != 0
    ):
        raise HarnessError(f"target {target['container']} did not clean-start")
    if inspect["Image"] != target["expected_image_id"]:
        raise HarnessError(
            f"target image mismatch: expected {target['expected_image_id']}, got {inspect['Image']}"
        )
    started = state.get("StartedAt")
    if prior_started is not None and started == prior_started:
        raise HarnessError("reset_command did not produce a clean container restart")
    return inspect, verify_cold_files(target)


def docker_cgroup(container: str, env: dict[str, str]) -> dict[str, Any]:
    script = (
        "read current < /sys/fs/cgroup/memory.current; "
        "read peak < /sys/fs/cgroup/memory.peak; "
        "read swap < /sys/fs/cgroup/memory.swap.current; "
        "read maximum < /sys/fs/cgroup/memory.max; "
        "anon=0; file=0; while read k v; do case $k in anon) anon=$v;; file) file=$v;; esac; done < /sys/fs/cgroup/memory.stat; "
        "printf '%s %s %s %s %s %s\\n' $current $peak $swap $anon $file $maximum; "
        "while read k v; do printf '%s=%s ' $k $v; done < /sys/fs/cgroup/memory.events"
    )
    lines = run_checked(
        ["docker", "exec", container, "sh", "-c", script], env=env
    ).splitlines()
    current_raw, peak_raw, swap_raw, anon_raw, file_raw, maximum_raw = lines[0].split()
    current, peak, swap, anon, filemem = (
        int(value) for value in (current_raw, peak_raw, swap_raw, anon_raw, file_raw)
    )
    events = {
        key: int(value)
        for key, value in (part.split("=", 1) for part in " ".join(lines[1:]).split())
    }
    return {
        "total": current,
        "current": current,
        "peak": peak,
        "swap": swap,
        "anon": anon,
        "file": filemem,
        "max": int(maximum_raw) if maximum_raw != "max" else None,
        "events": events,
    }


def process_memory(pid: int) -> dict[str, Any]:
    result = subprocess.run(
        ["ps", "-o", "rss=,vsz=", "-p", str(pid)], text=True, capture_output=True
    )
    fields = result.stdout.split()
    value: dict[str, Any] = {
        "pid": pid,
        "rss": int(fields[0]) * 1024 if len(fields) >= 2 else None,
        "vsz": int(fields[1]) * 1024 if len(fields) >= 2 else None,
    }
    rollup = Path(f"/proc/{pid}/smaps_rollup")
    if rollup.exists():
        smaps = {}
        for line in rollup.read_text(encoding="utf-8").splitlines():
            if ":" in line:
                key, rest = line.split(":", 1)
                token = rest.strip().split()
                if token and token[0].isdigit():
                    smaps[key] = int(token[0]) * 1024
        value["smaps_rollup"] = smaps
    else:
        value["smaps_rollup"] = None
    cgroup = Path(f"/proc/{pid}/cgroup")
    value["cgroup"] = (
        cgroup.read_text(encoding="utf-8").strip() if cgroup.exists() else None
    )
    return value


def scenario_env(scenario: dict[str, Any], output: Path) -> dict[str, str]:
    """Resolve declared env plus private, non-serialized LLM credentials."""
    declared = {
        key: value.replace("{output_dir}", str(output))
        for key, value in scenario.get("env", {}).items()
    }
    return {**declared, **private_llm_env(scenario)}


def container_memory_limit(
    inspect: dict[str, Any], cgroup: dict[str, Any] | None
) -> int:
    configured = int(inspect.get("HostConfig", {}).get("Memory", 0))
    return configured if configured > 0 else int((cgroup or {}).get("max") or 0)


def workload_identity(
    scenario: dict[str, Any],
    process: subprocess.Popen[bytes],
    env: dict[str, str],
    *,
    require_running: bool = True,
) -> dict[str, Any]:
    container = scenario.get("workload_container")
    if not container:
        return {
            "pid": process.pid,
            "container_id": None,
            "image_id": None,
            "running": process.poll() is None,
            "memory_limit_bytes": scenario["workload_memory_limit_bytes"],
        }
    inspect = docker_inspect(container, env)
    if inspect["Image"] != scenario["expected_workload_image_id"]:
        raise HarnessError(
            f"workload image mismatch: expected {scenario['expected_workload_image_id']}, got {inspect['Image']}"
        )
    pid = int(inspect["State"].get("Pid", 0))
    running = bool(inspect["State"].get("Running"))
    if require_running and (not running or pid <= 0):
        raise HarnessError(f"workload container {container} is not running")
    return {
        "pid": pid,
        "container_id": inspect["Id"],
        "image_id": inspect["Image"],
        "running": running,
        "memory_limit_bytes": int(inspect.get("HostConfig", {}).get("Memory", 0)),
    }


def sample(
    target: dict[str, Any],
    process: subprocess.Popen[bytes],
    scenario: dict[str, Any],
    env: dict[str, str],
    elapsed: float,
    *,
    require_workload_live: bool = True,
) -> dict[str, Any]:
    inspect = docker_inspect(target["container"], env)
    database = (
        docker_cgroup(target["container"], env)
        if inspect["State"].get("Running")
        else None
    )
    identity = workload_identity(
        scenario, process, env, require_running=require_workload_live
    )
    container = scenario.get("workload_container")
    workload_cgroup = (
        docker_cgroup(container, env) if container and identity["running"] else None
    )
    workload_limit = identity["memory_limit_bytes"]
    if container:
        workload_inspect = {
            "HostConfig": {"Memory": workload_limit},
        }
        workload_limit = container_memory_limit(workload_inspect, workload_cgroup)
    database_limit = container_memory_limit(inspect, database)
    return {
        "elapsed_seconds": round(elapsed, 3),
        "database": database,
        "database_state": {
            "running": inspect["State"].get("Running"),
            "oom": inspect["State"].get("OOMKilled"),
            "restarts": inspect.get("RestartCount", 0),
            "exit_code": inspect["State"].get("ExitCode"),
        },
        "workload_process": process_memory(identity["pid"])
        if identity["pid"] > 0
        else None,
        "workload_cgroup": workload_cgroup,
        "workload_state": identity,
        "configured_limits": {
            "database_bytes": database_limit,
            "workload_bytes": workload_limit,
        },
    }


def guard_reason(
    item: dict[str, Any], initial: dict[str, Any], trace: dict[str, Any]
) -> str | None:
    state = item["database_state"]
    if state["oom"] or not state["running"] or state["restarts"]:
        return "database_failure"
    database = item["database"]
    limits = item.get("configured_limits") or {}
    database_limit = int(limits.get("database_bytes", 0))
    if database_limit <= 0:
        raise HarnessError("database container must have a hard memory limit")
    fraction = float(trace.get("sampling", {}).get("abort_fraction", 0.85))
    if database["current"] >= database_limit * fraction:
        return "database_memory_guard"
    workload_limit = int(limits.get("workload_bytes", 0))
    if workload_limit <= 0:
        raise HarnessError("workload must have a hard memory limit")
    workload_usage = (
        item["workload_cgroup"]["current"]
        if item.get("workload_cgroup")
        else (item.get("workload_process") or {}).get("rss")
    )
    if item["workload_state"]["running"] and workload_usage is None:
        raise HarnessError("running workload memory usage is unavailable")
    if workload_usage is not None and workload_usage >= workload_limit * fraction:
        return "workload_memory_guard"
    if any(database["events"].get(key, 0) for key in ("oom", "oom_kill")):
        return "oom_event"
    return None


def _monitor_profiler(
    hook: dict[str, Any],
    workload: subprocess.Popen[bytes],
    target: dict[str, Any],
    scenario: dict[str, Any],
    initial: dict[str, Any],
    trace: dict[str, Any],
    output: Path,
    docker: dict[str, str],
    workload_env: dict[str, str],
    samples: list[dict[str, Any]],
    started: float,
) -> str | None:
    attach = hook["mode"] == "attach"
    item = sample(
        target,
        workload,
        scenario,
        docker,
        time.monotonic() - started,
        require_workload_live=attach,
    )
    samples.append(item)
    if reason := guard_reason(item, initial, trace):
        return reason
    workload_pid = item["workload_state"]["pid"] if attach else None
    database_pid = int(initial["State"].get("Pid", 0)) if attach else None
    profiler = start_profiler(
        hook,
        workload_pid,
        output,
        workload_env,
        database_pid,
    )
    try:
        while profiler.poll() is None:
            item = sample(
                target,
                workload,
                scenario,
                docker,
                time.monotonic() - started,
                require_workload_live=attach,
            )
            samples.append(item)
            if reason := guard_reason(item, initial, trace):
                return reason
            if time.monotonic() - started >= int(
                trace.get("sampling", {}).get("max_seconds", 600)
            ):
                return "timeout"
            time.sleep(int(trace.get("sampling", {}).get("interval_ms", 1000)) / 1000)
        if profiler.returncode != 0:
            return "profiler_failure"
        item = sample(
            target,
            workload,
            scenario,
            docker,
            time.monotonic() - started,
            require_workload_live=attach,
        )
        samples.append(item)
        return guard_reason(item, initial, trace)
    finally:
        stop_profiler(profiler)


def run_scenario(
    trace: dict[str, Any],
    scenario: dict[str, Any],
    output: Path,
    prior_started: str | None,
) -> tuple[dict[str, Any], str]:
    env = docker_env(trace)
    target = trace["targets"][scenario["target"]]
    initial, cold_files = reset_target(target, env, prior_started)
    started_at = initial["State"]["StartedAt"]
    workload_env = {**docker_env(trace), **scenario_env(scenario, output)}
    fd = os.open(
        output / f"{scenario['name']}.workload.log",
        os.O_WRONLY | os.O_CREAT | os.O_EXCL,
        0o600,
    )
    samples: list[dict[str, Any]] = []
    aborted = None
    started = time.monotonic()
    with os.fdopen(fd, "wb") as log:
        process = subprocess.Popen(
            scenario["command"],
            cwd=scenario.get("cwd") or ROOT,
            env=workload_env,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
        active: list[subprocess.Popen[bytes]] = []
        pause_host = not scenario.get("workload_container")
        try:
            if pause_host:
                process.send_signal(signal.SIGSTOP)
            for hook in hooks_for(scenario, "before"):
                aborted = _monitor_profiler(
                    hook,
                    process,
                    target,
                    scenario,
                    initial,
                    trace,
                    output,
                    env,
                    workload_env,
                    samples,
                    started,
                )
                if aborted:
                    break
            if not aborted:
                pid = workload_identity(scenario, process, env)["pid"]
                database_pid = int(initial["State"].get("Pid", 0))
                during_hooks = hooks_for(scenario, "during")
                active = [
                    start_profiler(hook, pid, output, workload_env, database_pid)
                    for hook in during_hooks
                ]
                startup_delay_ms = max(
                    (int(hook.get("startup_delay_ms", 0)) for hook in during_hooks),
                    default=0,
                )
                if startup_delay_ms > 0:
                    time.sleep(startup_delay_ms / 1000)
                if any(profiler.poll() not in (None, 0) for profiler in active):
                    aborted = "profiler_failure"
                if pause_host:
                    process.send_signal(signal.SIGCONT)
            while True:
                item = sample(
                    target, process, scenario, env, time.monotonic() - started
                )
                samples.append(item)
                if aborted or (aborted := guard_reason(item, initial, trace)):
                    break
                if any(profiler.poll() not in (None, 0) for profiler in active):
                    aborted = "profiler_failure"
                    break
                if process.poll() is not None:
                    break
                if time.monotonic() - started >= int(
                    trace.get("sampling", {}).get("max_seconds", 600)
                ):
                    aborted = "timeout"
                    break
                time.sleep(
                    int(trace.get("sampling", {}).get("interval_ms", 1000)) / 1000
                )
        finally:
            if pause_host and process.poll() is None:
                process.send_signal(signal.SIGCONT)
            for profiler in active:
                stop_profiler(profiler)
            stop_process(process)
        if not aborted:
            for hook in hooks_for(scenario, "after"):
                aborted = _monitor_profiler(
                    hook,
                    process,
                    target,
                    scenario,
                    initial,
                    trace,
                    output,
                    env,
                    workload_env,
                    samples,
                    started,
                )
                if aborted:
                    break
    after, after_cold = reset_target(target, env, started_at)
    artifacts = collect_profiler_artifacts(scenario, trace, output)
    lane = trace["run"]["evidence_lane"]
    result = {
        "name": scenario["name"],
        "target": scenario["target"],
        "target_kind": target["kind"],
        "command": scenario["command"],
        "exit_code": process.returncode,
        "aborted": aborted,
        "verdict": "pass" if not aborted and process.returncode == 0 else "fail",
        "canonical": lane == "faithful",
        "evidence_lane": lane,
        "allocator": trace["run"]["allocator"],
        "profiler_artifacts": artifacts,
        "cold_files_before": cold_files,
        "cold_files_after": after_cold,
        "samples": samples,
        "configured_limits": samples[0]["configured_limits"],
        "database_identity": {
            "container_id": initial["Id"],
            "image_id": initial["Image"],
            "name": initial["Name"],
        },
        "clean_restart": {
            "container_id": after["Id"],
            "started_at": after["State"]["StartedAt"],
            "image_id": after["Image"],
        },
    }
    return result, after["State"]["StartedAt"]
