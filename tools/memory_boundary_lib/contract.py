"""Trace contract and immutable safety validation for memory-boundary runs."""

from __future__ import annotations

import hashlib
import json
import os
import re
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parents[2]
SCHEMA_VERSION = 2
TARGET_KINDS = {"real_helixdb", "helixdb_mock"}
EVIDENCE_LANES = {"faithful", "diagnostic"}
ALLOCATORS = {"system", "mimalloc", "dhat", "jemalloc"}
PROFILER_CLASSES = {"cpu", "heap"}
PROFILER_PHASES = {"before", "during", "after"}
PROFILER_MODES = {"attach", "postprocess"}
PROFILER_TARGETS = {"workload", "database"}
PROFILER_STOP_SIGNALS = {"TERM", "INT"}
SECRET_PARTS = ("key", "token", "password", "secret", "credential")
FAITHFUL_LLM_PROVIDER = "cerebras"
FAITHFUL_LLM_MODEL = "gpt-oss-120b"


class HarnessError(RuntimeError):
    """The trace or runtime violated a fail-closed harness invariant."""


def checked_command(value: Any, field: str) -> list[str]:
    if (
        not isinstance(value, list)
        or not value
        or not all(isinstance(item, str) and item for item in value)
    ):
        raise HarnessError(f"{field} must be a non-empty argv list")
    return value


def secret_shaped(command: list[str]) -> bool:
    return any(
        any(
            f"{part}=" in arg.lower() or f"--{part}" in arg.lower()
            for part in SECRET_PARTS
        )
        for arg in command
    )


def secret_keys(value: Any) -> bool:
    if isinstance(value, dict):
        return any(
            any(part in str(key).lower() for part in SECRET_PARTS) or secret_keys(item)
            for key, item in value.items()
        )
    if isinstance(value, list):
        return any(secret_keys(item) for item in value)
    return False


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            digest.update(block)
    return digest.hexdigest()


def trace_digest(trace: dict[str, Any]) -> str:
    encoded = json.dumps(trace, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as stream:
        value = json.load(stream)
    if not isinstance(value, dict):
        raise HarnessError(f"{path} must contain a JSON object")
    return value


def output_relative_path(raw: str) -> Path:
    if raw.startswith("{output_dir}/"):
        raw = raw.removeprefix("{output_dir}/")
    path = Path(raw)
    if not raw or path.is_absolute() or ".." in path.parts:
        raise HarnessError(f"artifact path must be relative to output_dir: {raw}")
    return path


def _validate_run(run: Any) -> None:
    if not isinstance(run, dict):
        raise HarnessError("run must be an object")
    required = (
        "run_id",
        "git_commit",
        "candidate_binary_path",
        "candidate_binary_sha256",
        "build_profile",
        "evidence_lane",
        "allocator",
    )
    missing = [field for field in required if not run.get(field)]
    if missing:
        raise HarnessError(f"run is missing required metadata: {', '.join(missing)}")
    if not re.fullmatch(r"[A-Za-z0-9._-]+", run["run_id"]):
        raise HarnessError("run.run_id must be filesystem-safe")
    if not re.fullmatch(r"[0-9a-f]{40}", run["git_commit"]):
        raise HarnessError("run.git_commit must be an exact 40-character commit")
    if not re.fullmatch(r"[0-9a-f]{64}", run["candidate_binary_sha256"]):
        raise HarnessError("run.candidate_binary_sha256 must be SHA-256")
    if run["evidence_lane"] not in EVIDENCE_LANES:
        raise HarnessError("run.evidence_lane must be faithful or diagnostic")
    if run["allocator"] not in ALLOCATORS:
        raise HarnessError("run.allocator must be system, mimalloc, dhat or jemalloc")


def _validate_hook(hook: Any, scenario: str, lane: str, ids: set[str]) -> None:
    hook_id = hook.get("id") if isinstance(hook, dict) else None
    if not hook_id or hook_id in ids:
        raise HarnessError(f"scenarios.{scenario} profiler hooks need unique ids")
    ids.add(hook_id)
    if (
        hook.get("class") not in PROFILER_CLASSES
        or hook.get("phase") not in PROFILER_PHASES
    ):
        raise HarnessError(
            f"scenarios.{scenario}.profilers.{hook_id} has invalid class/phase"
        )
    if hook.get("mode") not in PROFILER_MODES:
        raise HarnessError(f"scenarios.{scenario}.profilers.{hook_id} has invalid mode")
    attach_to = hook.get("attach_to", "workload")
    if attach_to not in PROFILER_TARGETS:
        raise HarnessError(
            f"scenarios.{scenario}.profilers.{hook_id} has invalid attach_to"
        )
    if hook.get("stop_signal", "TERM") not in PROFILER_STOP_SIGNALS:
        raise HarnessError(
            f"scenarios.{scenario}.profilers.{hook_id} has invalid stop_signal"
        )
    startup_delay_ms = hook.get("startup_delay_ms", 0)
    if (
        not isinstance(startup_delay_ms, int)
        or isinstance(startup_delay_ms, bool)
        or not 0 <= startup_delay_ms <= 5_000
    ):
        raise HarnessError(
            f"scenarios.{scenario}.profilers.{hook_id} startup_delay_ms must be 0..5000"
        )
    command = checked_command(
        hook.get("command"), f"scenarios.{scenario}.profilers.{hook_id}.command"
    )
    if secret_shaped(command):
        raise HarnessError(
            f"scenarios.{scenario}.profilers.{hook_id} command embeds a secret-shaped argument"
        )
    required_pid = "{database_pid}" if attach_to == "database" else "{workload_pid}"
    any_pid = any(
        placeholder in arg
        for arg in command
        for placeholder in ("{workload_pid}", "{database_pid}")
    )
    if hook["mode"] == "attach" and (
        hook["phase"] == "after" or not any(required_pid in arg for arg in command)
    ):
        raise HarnessError(
            f"scenarios.{scenario}.profilers.{hook_id} attach hook requires a live {attach_to} PID before/during"
        )
    if hook["mode"] == "postprocess" and (
        hook["phase"] != "after" or any_pid
    ):
        raise HarnessError(
            f"scenarios.{scenario}.profilers.{hook_id} postprocess hook is after-only and PID-free"
        )
    if not hook.get("profiler_name") or not hook.get("profiler_version"):
        raise HarnessError(
            f"scenarios.{scenario}.profilers.{hook_id} requires profiler name/version"
        )
    config = hook.get("profiler_config")
    if not isinstance(config, dict) or secret_keys(config):
        raise HarnessError(
            f"scenarios.{scenario}.profilers.{hook_id} requires secret-free profiler_config"
        )
    artifacts = hook.get("artifacts")
    if (
        not isinstance(artifacts, list)
        or not artifacts
        or not all(isinstance(item, str) and item for item in artifacts)
    ):
        raise HarnessError(
            f"scenarios.{scenario}.profilers.{hook_id}.artifacts must be non-empty"
        )
    for raw in artifacts:
        output_relative_path(raw)
    if hook["class"] == "heap" and lane != "diagnostic":
        raise HarnessError(
            f"scenarios.{scenario}: heap allocation profiling requires diagnostic evidence lane"
        )


def validate_trace(trace: dict[str, Any]) -> None:
    if trace.get("schema_version") != SCHEMA_VERSION:
        raise HarnessError(f"trace schema_version must be {SCHEMA_VERSION}")
    _validate_run(trace.get("run"))
    parsed = urlparse(trace.get("docker_host") or "")
    if parsed.scheme != "tcp" or parsed.hostname not in {"127.0.0.1", "localhost"}:
        raise HarnessError(
            "docker_host must be a loopback TCP disposable daemon, never the default socket"
        )
    if not trace.get("disposable_daemon_id"):
        raise HarnessError("disposable_daemon_id is required")
    sampling = trace.get("sampling", {})
    abort = sampling.get("abort_fraction", 0.85)
    if not isinstance(abort, (int, float)) or not 0.1 <= float(abort) <= 0.85:
        raise HarnessError("sampling.abort_fraction must be between 0.10 and 0.85")
    if (
        int(sampling.get("interval_ms", 1000)) < 100
        or int(sampling.get("max_seconds", 600)) <= 0
    ):
        raise HarnessError("sampling interval must be >=100ms and max_seconds positive")
    targets = trace.get("targets")
    if not isinstance(targets, dict) or not targets:
        raise HarnessError("targets must be a non-empty object")
    for name, target in targets.items():
        if (
            target.get("kind") not in TARGET_KINDS
            or not target.get("container")
            or not target.get("expected_image_id")
        ):
            raise HarnessError(
                f"targets.{name} requires valid kind, container and expected_image_id"
            )
        if int(target.get("port", 0)) == 6970:
            raise HarnessError("refusing production HelixDB port 6970")
        for field in ("reset_command", "ready_command"):
            command = checked_command(target.get(field), f"targets.{name}.{field}")
            if secret_shaped(command):
                raise HarnessError(
                    f"targets.{name}.{field} embeds a secret-shaped argument"
                )
        cold_files = target.get("cold_files", [])
        if target["kind"] == "real_helixdb" and not cold_files:
            raise HarnessError(
                f"targets.{name}.cold_files is required for real HelixDB"
            )
        if any(
            not isinstance(row, dict) or not row.get("path") or not row.get("sha256")
            for row in cold_files
        ):
            raise HarnessError(
                f"targets.{name}.cold_files rows require path and sha256"
            )
    scenarios = trace.get("scenarios")
    if not isinstance(scenarios, list) or not scenarios:
        raise HarnessError("scenarios must be a non-empty list")
    names: set[str] = set()
    for scenario in scenarios:
        name = scenario.get("name")
        if not name or name in names or scenario.get("target") not in targets:
            raise HarnessError("scenarios require unique names and known targets")
        names.add(name)
        command = checked_command(scenario.get("command"), f"scenarios.{name}.command")
        env = scenario.get("env", {})
        if (
            secret_shaped(command)
            or not isinstance(env, dict)
            or not all(isinstance(key, str) and isinstance(value, str) for key, value in env.items())
            or secret_keys(env)
        ):
            raise HarnessError(
                f"scenarios.{name} contains invalid or secret-shaped command/env"
            )
        target = targets[scenario["target"]]
        llm_runtime = scenario.get("llm_runtime")
        if llm_runtime is not None and (
            not isinstance(llm_runtime, dict)
            or set(llm_runtime) != {"provider", "model"}
            or llm_runtime.get("provider") != FAITHFUL_LLM_PROVIDER
            or llm_runtime.get("model") != FAITHFUL_LLM_MODEL
        ):
            raise HarnessError(
                f"scenarios.{name}.llm_runtime must pin "
                f"{FAITHFUL_LLM_PROVIDER}/{FAITHFUL_LLM_MODEL}"
            )
        if (
            trace["run"]["evidence_lane"] == "faithful"
            and target["kind"] == "real_helixdb"
            and "HELIXIR_CONFIG" in env
            and llm_runtime is None
        ):
            raise HarnessError(
                f"scenarios.{name} uses a partial HELIXIR_CONFIG in a faithful "
                "real-database lane and must declare llm_runtime"
            )
        if scenario.get("workload_container") == target["container"]:
            raise HarnessError(
                f"scenario {name} must separate workload and database cgroups"
            )
        if scenario.get("workload_container") and not scenario.get(
            "expected_workload_image_id"
        ):
            raise HarnessError(f"scenario {name} requires expected_workload_image_id")
        if not scenario.get("workload_container"):
            workload_limit = scenario.get("workload_memory_limit_bytes")
            if not isinstance(workload_limit, int) or workload_limit <= 0:
                raise HarnessError(
                    f"scenario {name} host workload requires positive workload_memory_limit_bytes"
                )
        profilers = scenario.get("profilers", {"enabled": False, "hooks": []})
        if not isinstance(profilers, dict) or not isinstance(
            profilers.get("enabled", False), bool
        ):
            raise HarnessError(f"scenarios.{name}.profilers must be an object")
        hooks = profilers.get("hooks", [])
        if not isinstance(hooks, list) or (
            hooks and not profilers.get("enabled", False)
        ):
            raise HarnessError(
                f"scenarios.{name} profiler hooks are invalid or disabled by default"
            )
        ids: set[str] = set()
        for hook in hooks:
            _validate_hook(hook, name, trace["run"]["evidence_lane"], ids)
    artifacts = trace.get("private_artifacts", [])
    if not isinstance(artifacts, list) or not all(
        isinstance(item, str) for item in artifacts
    ):
        raise HarnessError("private_artifacts must be a list")
    for raw in artifacts:
        output_relative_path(raw)


def docker_env(trace: dict[str, Any]) -> dict[str, str]:
    return {**os.environ, "DOCKER_HOST": trace["docker_host"]}
