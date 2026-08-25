#!/usr/bin/env python3
"""Safety and result-contract tests for the HelixDB profiling automation."""

from __future__ import annotations

import json
import os
import stat
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools.profiling import helixdb_target
from tools.profiling.report_summary import query_summary, write_private
from tools.profiling.validate_samply import SamplyValidationError, validate


class TargetSafetyTests(unittest.TestCase):
    def test_only_disposable_profile_names_are_accepted(self) -> None:
        helixdb_target.safe_name("helixir-profile-db", "container")
        for name in ("helix-helixir-local-bench_app", "production", "helixir-profile-"):
            with self.subTest(name=name), self.assertRaises(helixdb_target.TargetError):
                helixdb_target.safe_name(name, "container")

    def test_daemon_validation_requires_loopback_and_exact_identity(self) -> None:
        with (
            patch.dict(os.environ, {"DOCKER_HOST": "tcp://127.0.0.1:12375"}),
            patch.object(helixdb_target, "run", return_value="expected"),
        ):
            helixdb_target.validate_daemon("expected")
        with patch.dict(os.environ, {"DOCKER_HOST": "unix:///var/run/docker.sock"}):
            with self.assertRaisesRegex(helixdb_target.TargetError, "loopback"):
                helixdb_target.validate_daemon("expected")


class ResultContractTests(unittest.TestCase):
    def test_samply_validation_requires_samples_frames_and_symbols(self) -> None:
        valid = {
            "meta": {"symbolicated": True},
            "threads": [
                {
                    "samples": {"length": 3},
                    "frameTable": {"length": 2},
                    "funcTable": {"length": 2},
                }
            ],
        }
        self.assertEqual(validate(valid)["samples"], 3)
        for mutation in (
            {"meta": {"symbolicated": True}, "threads": []},
            {
                "meta": {"symbolicated": False},
                "threads": [
                    {
                        "samples": {"length": 1},
                        "frameTable": {"length": 1},
                        "funcTable": {"length": 1},
                    }
                ],
            },
        ):
            with self.subTest(mutation=mutation), self.assertRaises(
                SamplyValidationError
            ):
                validate(mutation)

    def test_query_summary_never_copies_parameter_values(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "query-trace.jsonl"
            path.write_text(
                json.dumps(
                    {
                        "query": "searchMemory",
                        "parameter_keys": ["query"],
                        "status": "ok",
                        "duration_micros": 42,
                        "forbidden_value": "private memory text",
                    }
                )
                + "\n",
                encoding="utf-8",
            )
            summary = query_summary(path)
            encoded = json.dumps(summary)
            self.assertIn("searchMemory", encoded)
            self.assertNotIn("private memory text", encoded)
            self.assertNotIn("parameter_keys", encoded)

    @unittest.skipUnless(os.name == "posix", "POSIX permission contract")
    def test_private_result_is_created_with_mode_0600(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "nested" / "summary.json"
            write_private(path, {"verdict": "pass"})
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)


class RepositoryBudgetTests(unittest.TestCase):
    def test_profiling_automation_modules_stay_below_500_lines(self) -> None:
        root = Path(__file__).resolve().parents[1]
        paths = list((root / "tools" / "profiling").rglob("*.py"))
        oversized = {
            str(path.relative_to(root)): len(path.read_text(encoding="utf-8").splitlines())
            for path in paths
            if len(path.read_text(encoding="utf-8").splitlines()) > 500
        }
        self.assertEqual(oversized, {})


if __name__ == "__main__":
    unittest.main()
