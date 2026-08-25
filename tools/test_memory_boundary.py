#!/usr/bin/env python3
"""Model-free tests for the fail-closed memory-boundary package."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from tools import memory_boundary_lib as mb
from tools.memory_boundary_lib.profiling import profiler_command, start_profiler


def trace() -> dict:
    return {
        "schema_version": 2,
        "run": {
            "run_id": "unit-run",
            "git_commit": "a" * 40,
            "candidate_binary_path": "/private/candidate",
            "candidate_binary_sha256": "b" * 64,
            "build_profile": "release",
            "evidence_lane": "faithful",
            "allocator": "system",
        },
        "docker_host": "tcp://127.0.0.1:12375",
        "disposable_daemon_id": "disposable-daemon",
        "sampling": {"abort_fraction": 0.85, "interval_ms": 100, "max_seconds": 30},
        "targets": {
            "mock": {
                "kind": "helixdb_mock",
                "container": "boundary-mock",
                "expected_image_id": "sha256:mock",
                "port": 17970,
                "reset_command": ["docker", "restart", "boundary-mock"],
                "ready_command": ["docker", "inspect", "boundary-mock"],
            }
        },
        "scenarios": [
            {
                "name": "scalar-count",
                "target": "mock",
                "command": [sys.executable, "-c", "print('ok')"],
                "workload_memory_limit_bytes": 128 * 1024 * 1024,
            }
        ],
    }


def hook(kind: str = "cpu", mode: str = "attach", phase: str = "during") -> dict:
    command = ["profiler", "--out", "{output_dir}/profiles/result"]
    if mode == "attach":
        command.extend(["--pid", "{workload_pid}"])
    return {
        "id": f"{kind}-{mode}-{phase}",
        "class": kind,
        "mode": mode,
        "phase": phase,
        "command": command,
        "artifacts": ["profiles/result"],
        "profiler_name": "test-profiler",
        "profiler_version": "1.2.3",
        "profiler_config": {"frequency_hz": 99},
    }


def database_hook() -> dict:
    value = hook()
    value["id"] = "cpu-database-during"
    value["attach_to"] = "database"
    value["command"][-1] = "{database_pid}"
    return value


class TraceValidationTests(unittest.TestCase):
    def test_harness_modules_stay_within_repository_budget(self) -> None:
        paths = [Path(__file__).with_name("memory_boundary.py")]
        paths.extend(
            sorted(Path(__file__).with_name("memory_boundary_lib").glob("*.py"))
        )
        paths.append(Path(__file__))
        oversized = {
            str(path): len(path.read_text(encoding="utf-8").splitlines())
            for path in paths
            if len(path.read_text(encoding="utf-8").splitlines()) > 500
        }
        self.assertEqual(oversized, {}, f"Python modules exceed 500 lines: {oversized}")

    def test_accepts_minimal_disposable_trace(self) -> None:
        mb.validate_trace(trace())

    def test_requires_complete_reproducibility_metadata(self) -> None:
        for field in (
            "run_id",
            "git_commit",
            "candidate_binary_sha256",
            "build_profile",
            "evidence_lane",
            "allocator",
        ):
            value = trace()
            value["run"].pop(field)
            with (
                self.subTest(field=field),
                self.assertRaisesRegex(mb.HarnessError, "required metadata"),
            ):
                mb.validate_trace(value)

    def test_evidence_lane_and_allocator_are_independent(self) -> None:
        value = trace()
        value["run"]["allocator"] = "mimalloc"
        mb.validate_trace(value)
        value["run"]["evidence_lane"] = "diagnostic"
        value["run"]["allocator"] = "dhat"
        mb.validate_trace(value)

    def test_refuses_production_port_or_non_disposable_docker(self) -> None:
        value = trace()
        value["targets"]["mock"]["port"] = 6970
        with self.assertRaisesRegex(mb.HarnessError, "production"):
            mb.validate_trace(value)
        for host in ("unix:///var/run/docker.sock", "tcp://10.0.0.2:2375"):
            value = trace()
            value["docker_host"] = host
            with (
                self.subTest(host=host),
                self.assertRaisesRegex(mb.HarnessError, "loopback"),
            ):
                mb.validate_trace(value)

    def test_abort_threshold_cannot_exceed_85_percent(self) -> None:
        value = trace()
        value["sampling"]["abort_fraction"] = 0.851
        with self.assertRaisesRegex(mb.HarnessError, "0.85"):
            mb.validate_trace(value)

    def test_real_database_requires_cold_file_contract(self) -> None:
        value = trace()
        value["targets"]["mock"]["kind"] = "real_helixdb"
        with self.assertRaisesRegex(mb.HarnessError, "cold_files"):
            mb.validate_trace(value)

    def test_workload_container_is_separate_and_exact(self) -> None:
        value = trace()
        value["scenarios"][0]["workload_container"] = "boundary-mock"
        value["scenarios"][0]["expected_workload_image_id"] = "sha256:mock"
        with self.assertRaisesRegex(mb.HarnessError, "separate"):
            mb.validate_trace(value)
        value = trace()
        value["scenarios"][0]["workload_container"] = "helixir-workload"
        with self.assertRaisesRegex(mb.HarnessError, "expected_workload_image_id"):
            mb.validate_trace(value)

    def test_host_workload_requires_explicit_positive_limit(self) -> None:
        value = trace()
        value["scenarios"][0].pop("workload_memory_limit_bytes")
        with self.assertRaisesRegex(mb.HarnessError, "workload_memory_limit_bytes"):
            mb.validate_trace(value)

    def test_rejects_secret_shaped_workload_inputs(self) -> None:
        value = trace()
        value["scenarios"][0]["env"] = {"API_TOKEN": "do-not-log"}
        with self.assertRaisesRegex(mb.HarnessError, "secret-shaped"):
            mb.validate_trace(value)

    def test_expands_private_output_placeholder_without_shell_evaluation(self) -> None:
        scenario = {
            "env": {
                "HELIXIR_QUERY_TRACE_PATH": "{output_dir}/query-trace.jsonl",
                "LITERAL": "unchanged",
            }
        }
        expanded = mb.scenario_env(scenario, Path("/private/profile-run"))
        self.assertEqual(
            expanded["HELIXIR_QUERY_TRACE_PATH"],
            "/private/profile-run/query-trace.jsonl",
        )
        self.assertEqual(expanded["LITERAL"], "unchanged")

class ProfilerContractTests(unittest.TestCase):
    def test_profiler_child_forces_private_umask(self) -> None:
        output = Path("/private/output")
        fake_process = SimpleNamespace()
        with (
            patch("tools.memory_boundary_lib.profiling.os.open", return_value=7),
            patch("tools.memory_boundary_lib.profiling.os.fdopen"),
            patch(
                "tools.memory_boundary_lib.profiling.subprocess.Popen",
                return_value=fake_process,
            ) as popen,
        ):
            start_profiler(hook(), 222, output, {"PATH": "/usr/bin"})
        self.assertEqual(popen.call_args.kwargs["umask"], 0o077)

    def test_profiler_stop_signal_is_explicitly_bounded(self) -> None:
        value = trace()
        selected = hook()
        selected["stop_signal"] = "INT"
        value["scenarios"][0]["profilers"] = {"enabled": True, "hooks": [selected]}
        mb.validate_trace(value)
        selected["stop_signal"] = "KILL"
        with self.assertRaisesRegex(mb.HarnessError, "stop_signal"):
            mb.validate_trace(value)

    def test_profiler_startup_delay_is_bounded(self) -> None:
        value = trace()
        selected = hook()
        selected["startup_delay_ms"] = 2_000
        value["scenarios"][0]["profilers"] = {"enabled": True, "hooks": [selected]}
        mb.validate_trace(value)
        selected["startup_delay_ms"] = 5_001
        with self.assertRaisesRegex(mb.HarnessError, "startup_delay_ms"):
            mb.validate_trace(value)

    def test_hooks_are_disabled_by_default(self) -> None:
        value = trace()
        value["scenarios"][0]["profilers"] = {"enabled": False, "hooks": [hook()]}
        with self.assertRaisesRegex(mb.HarnessError, "disabled by default"):
            mb.validate_trace(value)

    def test_heap_profile_requires_diagnostic_lane_not_specific_allocator(self) -> None:
        value = trace()
        value["scenarios"][0]["profilers"] = {"enabled": True, "hooks": [hook("heap")]}
        with self.assertRaisesRegex(mb.HarnessError, "diagnostic evidence lane"):
            mb.validate_trace(value)
        value["run"]["evidence_lane"] = "diagnostic"
        value["run"]["allocator"] = "system"
        mb.validate_trace(value)

    def test_attach_and_postprocess_lifetimes_are_explicit(self) -> None:
        value = trace()
        value["scenarios"][0]["profilers"] = {
            "enabled": True,
            "hooks": [hook(mode="attach", phase="after")],
        }
        with self.assertRaisesRegex(mb.HarnessError, "live workload PID"):
            mb.validate_trace(value)
        value = trace()
        bad = hook(mode="postprocess", phase="after")
        bad["command"].append("{workload_pid}")
        value["scenarios"][0]["profilers"] = {"enabled": True, "hooks": [bad]}
        with self.assertRaisesRegex(mb.HarnessError, "PID-free"):
            mb.validate_trace(value)
        value["scenarios"][0]["profilers"] = {
            "enabled": True,
            "hooks": [hook(mode="postprocess", phase="after")],
        }
        mb.validate_trace(value)

    def test_database_profiler_requires_and_expands_database_pid(self) -> None:
        value = trace()
        value["scenarios"][0]["profilers"] = {
            "enabled": True,
            "hooks": [database_hook()],
        }
        mb.validate_trace(value)
        command = profiler_command(
            database_hook(), 111, Path("/private/output"), database_pid=222
        )
        self.assertIn("222", command)
        self.assertNotIn("111", command)

        bad = database_hook()
        bad["command"][-1] = "{workload_pid}"
        value["scenarios"][0]["profilers"]["hooks"] = [bad]
        with self.assertRaisesRegex(mb.HarnessError, "database PID"):
            mb.validate_trace(value)

    def test_artifacts_must_be_inside_output_dir(self) -> None:
        value = trace()
        outside = hook()
        outside["artifacts"] = ["/tmp/outside.profile"]
        value["scenarios"][0]["profilers"] = {"enabled": True, "hooks": [outside]}
        with self.assertRaisesRegex(mb.HarnessError, "relative to output_dir"):
            mb.validate_trace(value)

    def test_container_hook_uses_real_host_pid_not_wrapper_pid(self) -> None:
        scenario = {
            "workload_container": "helixir-workload",
            "expected_workload_image_id": "sha256:workload",
        }
        wrapper = SimpleNamespace(pid=111, poll=lambda: None)
        inspect = {
            "Id": "container-id",
            "Image": "sha256:workload",
            "State": {"Running": True, "Pid": 222},
        }
        with patch(
            "tools.memory_boundary_lib.runtime.docker_inspect", return_value=inspect
        ):
            identity = mb.workload_identity(scenario, wrapper, {})
        self.assertEqual(identity["pid"], 222)
        self.assertNotEqual(identity["pid"], wrapper.pid)
        command = profiler_command(hook(), identity["pid"], Path("/private/output"))
        self.assertIn("222", command)
        self.assertNotIn("111", command)

    def test_postprocess_identity_does_not_require_live_workload(self) -> None:
        scenario = {
            "workload_container": "helixir-workload",
            "expected_workload_image_id": "sha256:workload",
        }
        wrapper = SimpleNamespace(pid=111, poll=lambda: 0)
        inspect = {
            "Id": "container-id",
            "Image": "sha256:workload",
            "State": {"Running": False, "Pid": 0},
        }
        with patch(
            "tools.memory_boundary_lib.runtime.docker_inspect", return_value=inspect
        ):
            identity = mb.workload_identity(
                scenario, wrapper, {}, require_running=False
            )
        self.assertFalse(identity["running"])
        self.assertEqual(identity["pid"], 0)

    def test_artifact_metadata_inherits_run_and_profiler_provenance(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            artifact = output / "profiles" / "result"
            artifact.parent.mkdir()
            artifact.write_bytes(b"profile")
            artifact.chmod(0o600)
            value = trace()
            value["scenarios"][0]["profilers"] = {"enabled": True, "hooks": [hook()]}
            records = mb.collect_profiler_artifacts(
                value["scenarios"][0], value, output
            )
            self.assertEqual(records[0]["run_id"], "unit-run")
            self.assertEqual(records[0]["git_commit"], "a" * 40)
            self.assertEqual(records[0]["candidate_binary_sha256"], "b" * 64)
            self.assertEqual(records[0]["allocator"], "system")
            self.assertEqual(records[0]["profiler_name"], "test-profiler")
            self.assertEqual(records[0]["trace_digest"], mb.trace_digest(value))

    def test_instruments_bundle_is_hashed_recursively_and_privately(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            output.chmod(0o700)
            bundle = output / "heap" / "allocations.trace"
            bundle.mkdir(parents=True)
            bundle.parent.chmod(0o700)
            bundle.chmod(0o700)
            payload = bundle / "run.data"
            payload.write_bytes(b"private allocation profile")
            payload.chmod(0o600)
            records = mb.private_metadata(output, ["heap/allocations.trace"])
            self.assertEqual(records[0]["kind"], "directory")
            self.assertEqual(records[0]["entries"], 1)
            self.assertEqual(records[0]["size"], len(b"private allocation profile"))
            self.assertEqual(len(records[0]["sha256"]), 64)

            payload.chmod(0o644)
            with self.assertRaisesRegex(mb.HarnessError, "confined"):
                mb.private_metadata(output, ["heap/allocations.trace"])


class GuardAndVerdictTests(unittest.TestCase):
    def item(
        self,
        database_current: int,
        *,
        workload_current: int = 100,
        database_limit: int = 1000,
        workload_limit: int = 2000,
        oom: int = 0,
    ) -> dict:
        return {
            "database_state": {"running": True, "oom": False, "restarts": 0},
            "database": {
                "current": database_current,
                "events": {"oom": oom, "oom_kill": 0},
            },
            "workload_state": {"running": True},
            "workload_process": {"rss": workload_current},
            "workload_cgroup": None,
            "configured_limits": {
                "database_bytes": database_limit,
                "workload_bytes": workload_limit,
            },
        }

    def test_distinct_database_and_workload_exact_boundaries(self) -> None:
        initial = {"HostConfig": {"Memory": 1000}}
        self.assertIsNone(mb.guard_reason(self.item(849), initial, trace()))
        self.assertEqual(
            mb.guard_reason(self.item(850), initial, trace()),
            "database_memory_guard",
        )
        self.assertEqual(
            mb.guard_reason(self.item(100, workload_current=1700), initial, trace()),
            "workload_memory_guard",
        )

    def test_missing_database_or_workload_limit_fails_closed(self) -> None:
        initial = {"HostConfig": {"Memory": 1000}}
        with self.assertRaisesRegex(mb.HarnessError, "database container"):
            mb.guard_reason(self.item(1, database_limit=0), initial, trace())
        with self.assertRaisesRegex(mb.HarnessError, "workload"):
            mb.guard_reason(self.item(1, workload_limit=0), initial, trace())

    def test_container_limit_prefers_host_config_then_cgroup_max(self) -> None:
        self.assertEqual(
            mb.container_memory_limit({"HostConfig": {"Memory": 123}}, {"max": 456}),
            123,
        )
        self.assertEqual(
            mb.container_memory_limit({"HostConfig": {"Memory": 0}}, {"max": 456}),
            456,
        )

    def test_report_metadata_exposes_both_limits(self) -> None:
        limits = {"database_bytes": 1000, "workload_bytes": 2000}
        metadata = mb.configured_limit_metadata(
            [{"name": "scalar-count", "configured_limits": limits}]
        )
        self.assertEqual(metadata["scalar-count"], limits)

    def test_oom_event_remains_distinct_from_memory_guards(self) -> None:
        initial = {"HostConfig": {"Memory": 1000}}
        self.assertEqual(
            mb.guard_reason(self.item(1, oom=1), initial, trace()), "oom_event"
        )

    def test_diagnostic_failure_does_not_change_canonical_but_command_fails(
        self,
    ) -> None:
        results = [
            {"evidence_lane": "faithful", "verdict": "pass"},
            {"evidence_lane": "diagnostic", "verdict": "fail"},
        ]
        self.assertEqual(mb.canonical_verdict(results), "pass")
        self.assertEqual(mb.command_exit_code(results), 1)
        self.assertEqual(mb.canonical_verdict([results[1]]), "not_applicable")


if __name__ == "__main__":
    unittest.main()
