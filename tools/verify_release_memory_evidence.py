#!/usr/bin/env python3
"""Verify redacted faithful real-HelixDB evidence for one release tag."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any


GIB = 1024**3
HEX_64 = re.compile(r"^[0-9a-f]{64}$")
HEX_40 = re.compile(r"^[0-9a-f]{40}$")
SHA_256 = re.compile(r"^sha256:[0-9a-f]{64}$")
FORBIDDEN_KEYS = {
    "candidate_binary_path",
    "command",
    "cwd",
    "docker_host",
    "private_artifacts",
    "raw_log",
    "request_body",
    "response_body",
    "trace",
}
FORBIDDEN_VALUE_FRAGMENTS = ("/Users/", "/home/", "\\Users\\", "Bearer ")


class EvidenceError(ValueError):
    """Release evidence is absent, stale, unsafe, or does not prove a pass."""


def _load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise EvidenceError(f"evidence is missing: {path}") from error
    except (OSError, json.JSONDecodeError) as error:
        raise EvidenceError(f"evidence is unreadable: {path}: {error}") from error
    if not isinstance(value, dict):
        raise EvidenceError("evidence root must be an object")
    return value


def _reject_private_material(value: Any, path: str = "$") -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            if key in FORBIDDEN_KEYS:
                raise EvidenceError(f"public evidence contains forbidden field {path}.{key}")
            _reject_private_material(child, f"{path}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            _reject_private_material(child, f"{path}[{index}]")
    elif isinstance(value, str):
        if any(fragment in value for fragment in FORBIDDEN_VALUE_FRAGMENTS):
            raise EvidenceError(f"public evidence contains a private path or credential at {path}")


def _hex(value: Any, label: str, *, prefixed: bool = False) -> str:
    pattern = SHA_256 if prefixed else HEX_64
    if not isinstance(value, str) or not pattern.fullmatch(value):
        raise EvidenceError(f"{label} must be a lowercase SHA-256 digest")
    return value


def _relative_path(repo_root: Path, raw: Any) -> Path:
    if not isinstance(raw, str) or not raw or "\\" in raw:
        raise EvidenceError("runtime_source.includes entries must be POSIX paths")
    relative = Path(raw)
    if relative.is_absolute() or ".." in relative.parts or relative.as_posix() != raw:
        raise EvidenceError(f"unsafe runtime source path: {raw!r}")
    resolved = (repo_root / relative).resolve()
    if resolved != repo_root and repo_root not in resolved.parents:
        raise EvidenceError(f"runtime source path escapes repository: {raw!r}")
    return relative


def runtime_source_fingerprint(repo_root: Path, includes: list[Any]) -> tuple[str, int]:
    """Hash non-ignored source files as path-NUL-content-NUL in lexical order."""

    repo_root = repo_root.resolve()
    normalized = [_relative_path(repo_root, raw) for raw in includes]
    if len(set(normalized)) != len(normalized):
        raise EvidenceError("runtime_source.includes contains duplicates")
    files: set[Path] = set()
    for relative in normalized:
        source = repo_root / relative
        if source.is_symlink():
            raise EvidenceError(f"runtime source cannot be a symlink: {relative}")
        if source.is_file():
            candidates = [source]
        elif source.is_dir():
            candidates = sorted(source.rglob("*"))
        else:
            raise EvidenceError(f"runtime source path is missing: {relative}")
        for candidate in candidates:
            if candidate.is_symlink():
                raise EvidenceError(
                    f"runtime source cannot contain a symlink: {candidate.relative_to(repo_root)}"
                )
            if candidate.is_file():
                files.add(candidate.relative_to(repo_root))
    files.difference_update(_git_ignored_files(repo_root, files))
    if not files:
        raise EvidenceError("runtime source path set is empty")
    digest = hashlib.sha256()
    for relative in sorted(files, key=lambda item: item.as_posix()):
        digest.update(relative.as_posix().encode("utf-8"))
        digest.update(b"\0")
        with (repo_root / relative).open("rb") as stream:
            while chunk := stream.read(1024 * 1024):
                digest.update(chunk)
        digest.update(b"\0")
    return digest.hexdigest(), len(files)


def _git_ignored_files(repo_root: Path, files: set[Path]) -> set[Path]:
    """Return ignored untracked files without excluding tracked source files.

    A release checkout or ``git archive`` contains only tracked files. Local
    faithful runs may also contain Python bytecode, ``.DS_Store`` and other
    ignored machine artifacts below an included directory. Those files are not
    candidate source and must not make the public fingerprint host-dependent.
    """

    if not files or not (repo_root / ".git").exists():
        return set()
    payload = b"".join(
        relative.as_posix().encode("utf-8") + b"\0"
        for relative in sorted(files, key=lambda item: item.as_posix())
    )
    try:
        result = subprocess.run(
            ["git", "-C", str(repo_root), "check-ignore", "-z", "--stdin"],
            input=payload,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
    except OSError as error:
        raise EvidenceError(
            f"cannot classify ignored runtime source files: {error}"
        ) from error
    if result.returncode not in {0, 1}:
        detail = result.stderr.decode("utf-8", errors="replace").strip()
        raise EvidenceError(
            f"cannot classify ignored runtime source files: {detail or result.returncode}"
        )
    return {
        Path(raw.decode("utf-8"))
        for raw in result.stdout.split(b"\0")
        if raw
    }


def _parse_observed_at(value: Any) -> dt.datetime:
    if not isinstance(value, str) or not value.endswith("Z"):
        raise EvidenceError("observed_at must be an RFC3339 UTC timestamp")
    try:
        return dt.datetime.fromisoformat(value[:-1] + "+00:00")
    except ValueError as error:
        raise EvidenceError("observed_at must be an RFC3339 UTC timestamp") from error


def verify_evidence(
    evidence_path: Path,
    repo_root: Path,
    release_tag: str,
    *,
    now: dt.datetime | None = None,
) -> dict[str, Any]:
    evidence = _load_json(evidence_path)
    _reject_private_material(evidence)
    if evidence.get("schema_version") != 1:
        raise EvidenceError("unsupported evidence schema_version")
    if evidence.get("release_tag") != release_tag or not re.fullmatch(
        r"v\d+\.\d+\.\d+", release_tag
    ):
        raise EvidenceError("evidence does not bind the requested release tag")
    if evidence.get("evidence_lane") != "faithful":
        raise EvidenceError("canonical release evidence must use the faithful lane")
    if evidence.get("canonical_verdict") != "pass":
        raise EvidenceError("canonical faithful verdict is not pass")
    if evidence.get("instrumentation") != "none":
        raise EvidenceError("instrumented evidence cannot pass a release")

    observed = _parse_observed_at(evidence.get("observed_at"))
    max_age_days = evidence.get("max_age_days")
    if not isinstance(max_age_days, int) or not 1 <= max_age_days <= 30:
        raise EvidenceError("max_age_days must be between 1 and 30")
    current = now or dt.datetime.now(dt.timezone.utc)
    if current.tzinfo is None:
        current = current.replace(tzinfo=dt.timezone.utc)
    if observed > current + dt.timedelta(minutes=5):
        raise EvidenceError("evidence timestamp is in the future")
    if current > observed + dt.timedelta(days=max_age_days):
        raise EvidenceError("faithful evidence is stale")

    source = evidence.get("runtime_source")
    if (
        not isinstance(source, dict)
        or source.get("algorithm") != "sha256-nonignored-path-nul-content-nul-v2"
    ):
        raise EvidenceError("unsupported runtime source fingerprint")
    includes = source.get("includes")
    if not isinstance(includes, list):
        raise EvidenceError("runtime_source.includes must be a list")
    actual_digest, actual_count = runtime_source_fingerprint(repo_root, includes)
    if _hex(source.get("sha256"), "runtime_source.sha256") != actual_digest:
        raise EvidenceError("runtime source fingerprint drifted after the faithful run")
    if source.get("file_count") != actual_count:
        raise EvidenceError("runtime source file set drifted after the faithful run")

    policy = evidence.get("limits_policy")
    expected_policy = {
        "max_database_bytes": 3 * GIB,
        "max_workload_bytes": GIB,
        "max_abort_fraction": 0.85,
    }
    if policy != expected_policy:
        raise EvidenceError("release memory policy must remain 3 GiB / 1 GiB / 0.85")

    required = evidence.get("required_scenarios")
    repeated = evidence.get("repeated_scenario")
    if not isinstance(required, list) or not required or not all(
        isinstance(item, str) and item for item in required
    ):
        raise EvidenceError("required_scenarios must be a non-empty string list")
    if not isinstance(repeated, str) or repeated not in required:
        raise EvidenceError("repeated_scenario must name a required scenario")

    runs = evidence.get("runs")
    if not isinstance(runs, list) or len(runs) < 2:
        raise EvidenceError("at least two faithful runs are required")
    seen_required: set[str] = set()
    repeated_count = 0
    trace_digests: set[str] = set()
    report_digests: set[str] = set()
    run_ids: set[str] = set()
    attestation: tuple[str, str, str, str, str] | None = None
    for run_index, run in enumerate(runs):
        if not isinstance(run, dict) or run.get("verdict") != "pass":
            raise EvidenceError(f"run {run_index} is not a pass")
        if run.get("evidence_lane") != "faithful" or run.get("instrumentation") != "none":
            raise EvidenceError(f"run {run_index} is not uninstrumented faithful evidence")
        if run.get("build_profile") not in {"release", "profiling"}:
            raise EvidenceError(f"run {run_index} does not use a release-optimized build")
        if run.get("allocator") != "mimalloc":
            raise EvidenceError(f"run {run_index} does not use the production database allocator")
        run_id = run.get("id")
        if not isinstance(run_id, str) or not run_id or run_id in run_ids:
            raise EvidenceError("faithful run ids must be unique non-empty strings")
        run_ids.add(run_id)
        tested_commit = run.get("tested_git_commit")
        if not isinstance(tested_commit, str) or not HEX_40.fullmatch(tested_commit):
            raise EvidenceError(f"run {run_index} tested_git_commit is invalid")
        report_digest = _hex(run.get("private_report_sha256"), f"run {run_index} report")
        trace_digest = _hex(run.get("trace_digest"), f"run {run_index} trace")
        candidate_source = _hex(
            run.get("tested_candidate_source_sha256"), f"run {run_index} candidate source"
        )
        candidate_binary = _hex(
            run.get("candidate_binary_sha256"), f"run {run_index} candidate binary"
        )
        backend_image = _hex(
            run.get("backend_image_id"), f"run {run_index} backend image", prefixed=True
        )
        if report_digest in report_digests or trace_digest in trace_digests:
            raise EvidenceError("faithful run checksums are duplicated")
        report_digests.add(report_digest)
        trace_digests.add(trace_digest)
        current_attestation = (
            tested_commit,
            candidate_source,
            candidate_binary,
            backend_image,
            run.get("cold_state_sha256"),
        )
        _hex(current_attestation[4], f"run {run_index} cold state")
        if attestation is None:
            attestation = current_attestation
        elif current_attestation != attestation:
            raise EvidenceError("repeat run checksum drifted from the primary run")

        scenarios = run.get("scenarios")
        if not isinstance(scenarios, list) or not scenarios:
            raise EvidenceError(f"run {run_index} has no scenarios")
        run_scenarios: set[str] = set()
        for scenario in scenarios:
            if not isinstance(scenario, dict):
                raise EvidenceError("scenario evidence must be an object")
            name = scenario.get("name")
            if not isinstance(name, str) or not name or name in run_scenarios:
                raise EvidenceError(f"run {run_index} scenario names must be unique")
            run_scenarios.add(name)
            if name in required:
                seen_required.add(name)
            if name == repeated:
                repeated_count += 1
            if scenario.get("verdict") != "pass" or scenario.get("exit_code") != 0:
                raise EvidenceError(f"scenario {name!r} did not pass")
            if scenario.get("aborted") is not False:
                raise EvidenceError(f"scenario {name!r} reached an abort guard")
            db_limit = scenario.get("database_limit_bytes")
            workload_limit = scenario.get("workload_limit_bytes")
            abort_fraction = scenario.get("abort_fraction")
            if not isinstance(db_limit, int) or not 0 < db_limit <= 3 * GIB:
                raise EvidenceError(f"scenario {name!r} exceeds the 3 GiB database limit")
            if not isinstance(workload_limit, int) or not 0 < workload_limit <= GIB:
                raise EvidenceError(f"scenario {name!r} exceeds the 1 GiB workload limit")
            if not isinstance(abort_fraction, (int, float)) or not 0 < abort_fraction <= 0.85:
                raise EvidenceError(f"scenario {name!r} exceeds the 0.85 abort fraction")
            db_peak = scenario.get("database_peak_bytes")
            workload_peak = scenario.get("workload_peak_bytes")
            if not isinstance(db_peak, int) or not 0 <= db_peak < db_limit * abort_fraction:
                raise EvidenceError(f"scenario {name!r} database peak is unsafe")
            if not isinstance(workload_peak, int) or not 0 <= workload_peak < workload_limit * abort_fraction:
                raise EvidenceError(f"scenario {name!r} workload peak is unsafe")
            if scenario.get("oom_events") != 0 or scenario.get("oom_kill_events") != 0:
                raise EvidenceError(f"scenario {name!r} reported an OOM event")
            if scenario.get("restarts") != 0 or scenario.get("clean_restart") is not True:
                raise EvidenceError(f"scenario {name!r} did not cleanly restart")
            if scenario.get("separate_measurements") is not True:
                raise EvidenceError(f"scenario {name!r} lacks separate memory measurements")
            cold_before = _hex(scenario.get("cold_checksum_before"), f"scenario {name} cold before")
            cold_after = _hex(scenario.get("cold_checksum_after"), f"scenario {name} cold after")
            image_before = _hex(
                scenario.get("backend_image_before"), f"scenario {name} image before", prefixed=True
            )
            image_after = _hex(
                scenario.get("backend_image_after"), f"scenario {name} image after", prefixed=True
            )
            if cold_before != cold_after or cold_before != run.get("cold_state_sha256"):
                raise EvidenceError(f"scenario {name!r} cold-state checksum drifted")
            if image_before != image_after or image_before != backend_image:
                raise EvidenceError(f"scenario {name!r} backend image checksum drifted")
        if run_index == 0 and not set(required).issubset(run_scenarios):
            raise EvidenceError("the primary faithful run does not cover every required scenario")

    if set(required) != seen_required:
        missing = sorted(set(required) - seen_required)
        raise EvidenceError(f"required faithful scenarios are missing: {missing}")
    if repeated_count < 2:
        raise EvidenceError("the representative faithful scenario was not repeated")
    return {
        "release_tag": release_tag,
        "runtime_source_sha256": actual_digest,
        "runtime_source_files": actual_count,
        "faithful_runs": len(runs),
        "verified_scenarios": sum(len(run["scenarios"]) for run in runs),
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--release-tag", required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        result = verify_evidence(args.evidence, args.repo_root, args.release_tag)
    except EvidenceError as error:
        print(f"release-memory-evidence: {error}", file=sys.stderr)
        return 2
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
