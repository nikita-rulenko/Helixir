"""Private artifacts, provenance and JSON/TSV reporting."""

from __future__ import annotations

import json
import hashlib
import os
import stat
from pathlib import Path
from typing import Any

from .contract import HarnessError, output_relative_path, sha256_file, trace_digest
from .profiling import hooks_for


def resolve_output_path(output: Path, raw: str) -> Path:
    output = output.resolve()
    path = (output / output_relative_path(raw)).resolve()
    if path != output and output not in path.parents:
        raise HarnessError(f"artifact escaped output_dir: {path}")
    return path


def _require_private_mode(path: Path, *, directory: bool) -> int:
    if path.is_symlink():
        raise HarnessError(f"private artifact cannot be a symlink: {path}")
    mode = stat.S_IMODE(path.stat().st_mode)
    if mode & 0o077:
        expected = "0700" if directory else "0600"
        raise HarnessError(
            f"private artifact is not confined like mode {expected}: {path} ({mode:o})"
        )
    return mode


def _directory_metadata(path: Path) -> tuple[str, int, int]:
    digest = hashlib.sha256()
    size = 0
    entries = 0
    _require_private_mode(path, directory=True)
    for child in sorted(path.rglob("*")):
        relative = child.relative_to(path).as_posix()
        if child.is_symlink():
            raise HarnessError(f"private artifact cannot contain a symlink: {child}")
        if child.is_dir():
            _require_private_mode(child, directory=True)
            digest.update(f"D\0{relative}\0".encode())
            continue
        if not child.is_file():
            raise HarnessError(f"private artifact contains a special file: {child}")
        _require_private_mode(child, directory=False)
        child_size = child.stat().st_size
        digest.update(f"F\0{relative}\0{child_size}\0{sha256_file(child)}\0".encode())
        size += child_size
        entries += 1
    return digest.hexdigest(), size, entries


def private_metadata(output: Path, paths: list[str]) -> list[dict[str, Any]]:
    records = []
    for raw in paths:
        path = resolve_output_path(output, raw)
        if path.is_dir():
            digest, size, entries = _directory_metadata(path)
            kind = "directory"
            mode = stat.S_IMODE(path.stat().st_mode)
        elif path.is_file() and not path.is_symlink():
            mode = _require_private_mode(path, directory=False)
            digest, size, entries = sha256_file(path), path.stat().st_size, 1
            kind = "file"
        else:
            raise HarnessError(
                f"private artifact is not a regular file or directory: {path}"
            )
        records.append(
            {
                "path": str(path),
                "kind": kind,
                "sha256": digest,
                "size": size,
                "entries": entries,
                "mode": f"{mode:04o}",
            }
        )
    return records


def profiler_inventory(trace: dict[str, Any]) -> list[dict[str, Any]]:
    rows = []
    for scenario in trace["scenarios"]:
        for phase in ("before", "during", "after"):
            for hook in hooks_for(scenario, phase):
                rows.append(
                    {
                        "scenario": scenario["name"],
                        "hook_id": hook["id"],
                        "name": hook["profiler_name"],
                        "version": hook["profiler_version"],
                        "config": hook["profiler_config"],
                        "class": hook["class"],
                        "mode": hook["mode"],
                        "phase": hook["phase"],
                    }
                )
    return rows


def run_metadata(trace: dict[str, Any]) -> dict[str, Any]:
    return {
        **trace["run"],
        "trace_digest": trace_digest(trace),
        "profilers": profiler_inventory(trace),
    }


def collect_profiler_artifacts(
    scenario: dict[str, Any], trace: dict[str, Any], output: Path
) -> list[dict[str, Any]]:
    records = []
    inherited = run_metadata(trace)
    for phase in ("before", "during", "after"):
        for hook in hooks_for(scenario, phase):
            for metadata in private_metadata(output, hook["artifacts"]):
                metadata.update(
                    {
                        **inherited,
                        "scenario": scenario["name"],
                        "hook_id": hook["id"],
                        "profiler_name": hook["profiler_name"],
                        "profiler_version": hook["profiler_version"],
                        "profiler_config": hook["profiler_config"],
                        "artifact_class": hook["class"],
                        "hook_mode": hook["mode"],
                        "hook_phase": hook["phase"],
                    }
                )
                records.append(metadata)
    return records


def canonical_verdict(results: list[dict[str, Any]]) -> str:
    faithful = [result for result in results if result["evidence_lane"] == "faithful"]
    if not faithful:
        return "not_applicable"
    return "pass" if all(result["verdict"] == "pass" for result in faithful) else "fail"


def command_exit_code(results: list[dict[str, Any]]) -> int:
    return 1 if any(result["verdict"] != "pass" for result in results) else 0


def configured_limit_metadata(results: list[dict[str, Any]]) -> dict[str, Any]:
    return {result["name"]: result["configured_limits"] for result in results}


def write_reports(output: Path, report: dict[str, Any]) -> None:
    json_path = output / "report.json"
    fd = os.open(json_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as stream:
        json.dump(report, stream, indent=2)
        stream.write("\n")
    tsv_path = output / "samples.tsv"
    fd = os.open(tsv_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as stream:
        stream.write(
            "scenario\ttarget\telapsed_s\tdb_limit\tworkload_limit\tdb_total\tdb_current\tdb_peak\tdb_anon\tdb_file\tdb_swap\tworkload_current\toom\toom_kill\tworkload_rss\tworkload_pss\tworkload_private_dirty\taborted\n"
        )
        for result in report["results"]:
            for item in result["samples"]:
                database = item["database"] or {}
                process = item["workload_process"] or {}
                smaps = process.get("smaps_rollup") or {}
                events = database.get("events") or {}
                limits = item["configured_limits"]
                workload = item.get("workload_cgroup") or {}
                values = (
                    result["name"],
                    result["target"],
                    item["elapsed_seconds"],
                    limits["database_bytes"],
                    limits["workload_bytes"],
                    database.get("total", ""),
                    database.get("current", ""),
                    database.get("peak", ""),
                    database.get("anon", ""),
                    database.get("file", ""),
                    database.get("swap", ""),
                    workload.get("current", process.get("rss", "")),
                    events.get("oom", ""),
                    events.get("oom_kill", ""),
                    process.get("rss", ""),
                    smaps.get("Pss", ""),
                    smaps.get("Private_Dirty", ""),
                    result["aborted"] or "",
                )
                stream.write("\t".join(str(value) for value in values) + "\n")
