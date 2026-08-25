"""Argv-only profiler hook lifecycle for the memory-boundary harness."""

from __future__ import annotations

import os
import signal
import subprocess
from pathlib import Path
from typing import Any


def hooks_for(scenario: dict[str, Any], phase: str) -> list[dict[str, Any]]:
    profilers = scenario.get("profilers", {})
    if not profilers.get("enabled", False):
        return []
    return [hook for hook in profilers.get("hooks", []) if hook["phase"] == phase]


def profiler_command(
    hook: dict[str, Any],
    workload_pid: int | None,
    output: Path,
    database_pid: int | None = None,
) -> list[str]:
    command = []
    for arg in hook["command"]:
        value = arg.replace("{output_dir}", str(output))
        if workload_pid is not None:
            value = value.replace("{workload_pid}", str(workload_pid))
        if database_pid is not None:
            value = value.replace("{database_pid}", str(database_pid))
        command.append(value)
    return command


def start_profiler(
    hook: dict[str, Any],
    workload_pid: int | None,
    output: Path,
    env: dict[str, str],
    database_pid: int | None = None,
) -> subprocess.Popen[bytes]:
    command = profiler_command(hook, workload_pid, output, database_pid)
    log_path = output / f"profiler-{hook['id']}.log"
    fd = os.open(log_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    log = os.fdopen(fd, "wb")
    # Profilers commonly create their own output files with mode 0644. Force a
    # private child umask so declared heap/CPU artifacts satisfy the 0600
    # contract without a post-hoc window where memory contents are world-readable.
    child = subprocess.Popen(
        command,
        env=env,
        stdout=log,
        stderr=subprocess.STDOUT,
        umask=0o077,
    )
    child._helixir_log = log  # type: ignore[attr-defined]
    child._helixir_stop_signal = hook.get("stop_signal", "TERM")  # type: ignore[attr-defined]
    return child


def stop_process(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    process.send_signal(signal.SIGTERM)
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


def stop_profiler(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is None:
        selected = getattr(process, "_helixir_stop_signal", "TERM")
        process.send_signal(signal.SIGINT if selected == "INT" else signal.SIGTERM)
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
    log = getattr(process, "_helixir_log", None)
    if log is not None:
        log.close()
