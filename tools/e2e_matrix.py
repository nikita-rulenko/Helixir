#!/usr/bin/env python3
"""Canonical inventory and safe runner for Helixir's ignored live E2E tests."""

from __future__ import annotations

import argparse
import atexit
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
TESTS_DIR = ROOT / "helixir" / "tests"
MANIFEST_PATH = ROOT / "tools" / "e2e_manifest.json"
TEST_PATTERN = re.compile(
    r'#\[ignore(?:\s*=\s*"([^"]*)")?\]\s*'
    r'(?:#\[[^\]]+\]\s*)*(?:async\s+)?fn\s+(\w+)'
)
TOPOLOGIES = {"current-schema", "fresh-store", "client-gate"}
INGEST_MODES = {"enabled", "disabled", "managed-by-test"}
CLEANUP_MODES = {"read-only", "fixture-guard", "disposable-database"}
GIB = 1024**3
DEFAULT_MIN_FREE_GIB = 20.0
DEFAULT_MAX_TARGET_GIB = 24.0


class ManifestError(RuntimeError):
    """The checked-in E2E manifest does not describe the Rust test tree."""


def _positive_gib(name: str, default: float) -> float:
    raw = os.environ.get(name, str(default))
    try:
        value = float(raw)
    except ValueError as error:
        raise ManifestError(f"{name} must be a positive number of GiB") from error
    if value <= 0:
        raise ManifestError(f"{name} must be a positive number of GiB")
    return value


def _tree_bytes(root: Path) -> int:
    total = 0
    if not root.exists():
        return total
    for directory, _, files in os.walk(root):
        for name in files:
            path = Path(directory) / name
            try:
                total += path.stat().st_size
            except FileNotFoundError:
                continue
    return total


def create_bounded_cargo_target() -> tuple[Path, float, float, int]:
    """Create one disposable Cargo cache and enforce release-gate headroom."""
    min_free_gib = _positive_gib("HELIXIR_E2E_MIN_FREE_GIB", DEFAULT_MIN_FREE_GIB)
    max_target_gib = _positive_gib(
        "HELIXIR_E2E_MAX_TARGET_GIB", DEFAULT_MAX_TARGET_GIB
    )
    temp_root = Path(
        os.environ.get("HELIXIR_E2E_TEMP_ROOT", tempfile.gettempdir())
    ).expanduser()
    temp_root.mkdir(parents=True, exist_ok=True)
    initial_free = shutil.disk_usage(temp_root).free
    required_free = (min_free_gib + max_target_gib) * GIB
    if initial_free < required_free:
        raise ManifestError(
            f"only {initial_free / GIB:.2f} GiB free on {temp_root}; "
            f"the E2E preflight requires {required_free / GIB:.2f} GiB "
            "(retained headroom plus bounded Cargo target)"
        )
    target = Path(tempfile.mkdtemp(prefix="helixir-e2e-cargo-", dir=temp_root))
    atexit.register(shutil.rmtree, target, True)
    print(
        f"E2E storage: target={target} initial_free={initial_free / GIB:.2f} GiB "
        f"max_target={max_target_gib:.2f} GiB",
        flush=True,
    )
    return target, min_free_gib, max_target_gib, initial_free


def check_storage_envelope(
    target: Path, min_free_gib: float, max_target_gib: float
) -> tuple[int, int]:
    """Fail closed before another test can exhaust the host or Docker disk."""
    target_bytes = _tree_bytes(target)
    free_bytes = shutil.disk_usage(target).free
    if target_bytes > max_target_gib * GIB:
        raise ManifestError(
            f"Cargo target reached {target_bytes / GIB:.2f} GiB; "
            f"limit is {max_target_gib:.2f} GiB"
        )
    if free_bytes < min_free_gib * GIB:
        raise ManifestError(
            f"free space fell to {free_bytes / GIB:.2f} GiB; "
            f"safety floor is {min_free_gib:.2f} GiB"
        )
    return target_bytes, free_bytes


def cleanup_cargo_target(target: Path | None) -> None:
    if target is not None:
        shutil.rmtree(target, ignore_errors=True)


def discover_ignored_tests() -> dict[str, str]:
    """Return ``target::test -> ignore reason`` for every ignored E2E test."""
    found: dict[str, str] = {}
    for path in sorted(TESTS_DIR.glob("*e2e.rs")):
        for match in TEST_PATTERN.finditer(path.read_text(encoding="utf-8")):
            key = f"{path.stem}::{match.group(2)}"
            if key in found:
                raise ManifestError(f"duplicate ignored test discovered: {key}")
            found[key] = match.group(1) or ""
    return found


def load_manifest() -> dict[str, Any]:
    with MANIFEST_PATH.open(encoding="utf-8") as stream:
        return json.load(stream)


def validate_manifest(manifest: dict[str, Any]) -> list[dict[str, Any]]:
    """Validate schema plus exact bidirectional coverage of ignored tests."""
    if manifest.get("schema_version") != 1:
        raise ManifestError("unsupported e2e manifest schema_version")
    tests = manifest.get("tests")
    if not isinstance(tests, list) or not tests:
        raise ManifestError("manifest tests must be a non-empty list")

    declared: dict[str, dict[str, Any]] = {}
    for row in tests:
        if not isinstance(row, dict):
            raise ManifestError("every manifest test must be an object")
        key = f"{row.get('target')}::{row.get('test')}"
        if key in declared:
            raise ManifestError(f"duplicate manifest entry: {key}")
        if row.get("topology") not in TOPOLOGIES:
            raise ManifestError(f"{key}: invalid topology {row.get('topology')!r}")
        environment = row.get("environment")
        if not isinstance(environment, dict):
            raise ManifestError(f"{key}: environment ownership is required")
        if environment.get("ingest_buffer") not in INGEST_MODES:
            raise ManifestError(f"{key}: invalid ingest_buffer ownership")
        for field in ("actor", "group", "retrieval_profile"):
            if not environment.get(field):
                raise ManifestError(f"{key}: environment.{field} is required")
        if row.get("cleanup") not in CLEANUP_MODES:
            raise ManifestError(f"{key}: invalid cleanup ownership")
        if not isinstance(row.get("requires"), list):
            raise ManifestError(f"{key}: requires must be a list")
        declared[key] = row

    discovered = discover_ignored_tests()
    missing = sorted(set(discovered) - set(declared))
    stale = sorted(set(declared) - set(discovered))
    reason_drift = sorted(
        key
        for key in set(discovered) & set(declared)
        if discovered[key] != declared[key].get("ignore_reason")
    )
    if missing or stale or reason_drift:
        details = []
        if missing:
            details.append("missing: " + ", ".join(missing))
        if stale:
            details.append("stale: " + ", ".join(stale))
        if reason_drift:
            details.append("ignore-reason drift: " + ", ".join(reason_drift))
        raise ManifestError("manifest drift detected; " + "; ".join(details))
    return tests


def build_environment(row: dict[str, Any], actor: str, group: str) -> dict[str, str]:
    """Build one suite's owned environment without leaking a prior suite's mode."""
    env = os.environ.copy()
    env.update(
        {
            "HELIX_E2E": "1",
            "HELIXIR_RBAC_ACTOR": actor,
            "HELIXIR_E2E_GROUP": group,
            "HELIXIR_RETRIEVAL_PROFILE": row["environment"]["retrieval_profile"],
            "HELIX_LLM_FALLBACK_CHAIN": "",
        }
    )
    ingest_mode = row["environment"]["ingest_buffer"]
    if ingest_mode == "enabled":
        env["HELIXIR_INGEST_BUFFER"] = "1"
    elif ingest_mode == "disabled":
        env.pop("HELIXIR_INGEST_BUFFER", None)
    return env


def ensure_disposable_target(host: str, port: int) -> None:
    if os.environ.get("HELIXIR_E2E_DISPOSABLE") != "1":
        raise ManifestError(
            "live execution requires HELIXIR_E2E_DISPOSABLE=1 on an isolated database"
        )
    if port == 6970:
        raise ManifestError("refusing the production HelixDB port 6970")
    if not host.strip():
        raise ManifestError("HELIX_HOST must identify the disposable HelixDB host")


def preflight_rbac(actor: str, group: str, env: dict[str, str]) -> None:
    """Prove permanent RBAC prerequisites through the product CLI, read-only."""
    command = [
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str(ROOT / "helixir" / "Cargo.toml"),
        "--bin",
        "helixir",
        "--",
        "rbac",
        "status",
        "--json",
    ]
    result = subprocess.run(command, cwd=ROOT, env=env, text=True, capture_output=True)
    if result.returncode != 0:
        raise ManifestError(f"RBAC preflight failed: {result.stderr.strip()}")
    try:
        policy = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise ManifestError(f"RBAC preflight returned invalid JSON: {error}") from error
    if not policy.get("enabled") or policy.get("migration_state") != "active":
        raise ManifestError("permanent RBAC must be enabled with migration_state=active")
    binding = policy.get("users", {}).get(actor, {})
    if "admin" not in binding.get("global_roles", []):
        raise ManifestError(f"E2E actor {actor!r} is not a global admin")
    if group not in policy.get("groups", {}):
        raise ManifestError(f"E2E group {group!r} is not active")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true", help="validate manifest drift only")
    mode.add_argument("--list", action="store_true", help="list the canonical matrix")
    mode.add_argument("--run", action="store_true", help="run one disposable topology")
    parser.add_argument("--topology", choices=sorted(TOPOLOGIES))
    parser.add_argument("--actor")
    parser.add_argument("--group")
    parser.add_argument("--only", action="append", default=[], metavar="TARGET::TEST")
    parser.add_argument(
        "--fresh-scenario",
        choices=("fresh", "legacy-upgrade", "interrupted-legacy"),
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    cargo_target: Path | None = None
    initial_free = 0
    peak_target = 0
    try:
        manifest = load_manifest()
        tests = validate_manifest(manifest)
        if args.check:
            print(f"E2E manifest OK: {len(tests)} ignored tests")
            return 0
        if args.list:
            for row in tests:
                print(
                    f"{row['target']}::{row['test']}\t{row['topology']}\t"
                    f"ingest={row['environment']['ingest_buffer']}\tcleanup={row['cleanup']}"
                )
            return 0
        if not args.topology:
            raise ManifestError("--run requires --topology")

        defaults = manifest["defaults"]
        actor = args.actor or os.environ.get("HELIXIR_RBAC_ACTOR") or defaults["actor_id"]
        group = args.group or os.environ.get("HELIXIR_E2E_GROUP") or defaults["group_id"]
        host = os.environ.get("HELIX_HOST", "127.0.0.1")
        port = int(os.environ.get("HELIX_PORT", "6969"))
        ensure_disposable_target(host, port)
        cargo_target, min_free_gib, max_target_gib, initial_free = (
            create_bounded_cargo_target()
        )

        selected = set(args.only)
        known = {f"{row['target']}::{row['test']}" for row in tests}
        unknown = selected - known
        if unknown:
            raise ManifestError("unknown --only test(s): " + ", ".join(sorted(unknown)))
        if args.topology == "fresh-store" and not args.fresh_scenario:
            raise ManifestError("fresh-store requires --fresh-scenario and one empty database")

        base_env = os.environ.copy()
        base_env.update(
            {
                "HELIX_HOST": host,
                "HELIX_PORT": str(port),
                "HELIXIR_RBAC_ACTOR": actor,
                "HELIXIR_E2E_GROUP": group,
                "CARGO_TARGET_DIR": str(cargo_target),
                "CARGO_INCREMENTAL": "0",
                "CARGO_PROFILE_TEST_DEBUG": "0",
            }
        )
        if args.topology != "fresh-store":
            preflight_rbac(actor, group, base_env)

        failures = []
        ran = 0
        for row in tests:
            key = f"{row['target']}::{row['test']}"
            if selected and key not in selected:
                print(f"SKIP {key}: not selected")
                continue
            if not selected and row.get("run_by_default") is False:
                print(f"SKIP {key}: explicit selection required")
                continue
            if row["topology"] != args.topology:
                print(f"SKIP {key}: requires topology {row['topology']}")
                continue
            env = build_environment(row, actor, group)
            env.update(
                {
                    "HELIX_HOST": host,
                    "HELIX_PORT": str(port),
                    "CARGO_TARGET_DIR": str(cargo_target),
                    "CARGO_INCREMENTAL": "0",
                    "CARGO_PROFILE_TEST_DEBUG": "0",
                }
            )
            if args.topology == "fresh-store":
                env["HELIX_E2E_FRESH"] = "1"
                env["HELIX_E2E_SCENARIO"] = args.fresh_scenario
            command = [
                "cargo",
                "test",
                "--manifest-path",
                str(ROOT / "helixir" / "Cargo.toml"),
                "--test",
                row["target"],
                row["test"],
                "--",
                "--ignored",
                "--nocapture",
                "--exact",
                "--test-threads=1",
            ]
            print(f"RUN  {key}", flush=True)
            ran += 1
            result = subprocess.run(command, cwd=ROOT, env=env)
            target_bytes, _ = check_storage_envelope(
                cargo_target, min_free_gib, max_target_gib
            )
            peak_target = max(peak_target, target_bytes)
            if result.returncode != 0:
                failures.append(key)
                print(f"FAIL {key}: exit {result.returncode}", flush=True)
            else:
                print(f"PASS {key}", flush=True)
        if ran == 0:
            raise ManifestError("selection matched no tests in the chosen topology")
        if failures:
            print("failed tests: " + ", ".join(failures), file=sys.stderr)
            return 1
        return 0
    except (ManifestError, OSError, ValueError) as error:
        print(f"e2e-matrix: {error}", file=sys.stderr)
        return 2
    finally:
        if cargo_target is not None:
            cleanup_cargo_target(cargo_target)
            final_free = shutil.disk_usage(cargo_target.parent).free
            print(
                f"E2E storage: peak_target={peak_target / GIB:.2f} GiB "
                f"free_initial={initial_free / GIB:.2f} GiB "
                f"free_after_cleanup={final_free / GIB:.2f} GiB",
                flush=True,
            )


if __name__ == "__main__":
    raise SystemExit(main())
