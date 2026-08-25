#!/usr/bin/env python3
"""Secret-hygiene regressions for faithful memory-boundary LLM auth."""

from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import memory_boundary_lib as mb


def scenario() -> dict:
    return {
        "env": {"HELIXIR_CONFIG": "/private/partial.toml"},
        "llm_runtime": {"provider": "cerebras", "model": "gpt-oss-120b"},
    }


class PrivateLlmCredentialTests(unittest.TestCase):
    def test_faithful_partial_config_requires_private_llm_runtime(self) -> None:
        value = {
            "schema_version": 2,
            "run": {
                "run_id": "credential-test",
                "git_commit": "a" * 40,
                "candidate_binary_path": "/private/candidate",
                "candidate_binary_sha256": "b" * 64,
                "build_profile": "release",
                "evidence_lane": "faithful",
                "allocator": "system",
            },
            "docker_host": "tcp://127.0.0.1:12375",
            "disposable_daemon_id": "disposable-daemon",
            "sampling": {
                "abort_fraction": 0.85,
                "interval_ms": 100,
                "max_seconds": 30,
            },
            "targets": {
                "real": {
                    "kind": "real_helixdb",
                    "container": "boundary-db",
                    "expected_image_id": "sha256:db",
                    "port": 17970,
                    "reset_command": ["docker", "restart", "boundary-db"],
                    "ready_command": ["docker", "inspect", "boundary-db"],
                    "cold_files": [
                        {"path": "/private/cold", "sha256": "c" * 64}
                    ],
                }
            },
            "scenarios": [
                {
                    "name": "daemon",
                    "target": "real",
                    "command": ["daemon-e2e"],
                    "workload_memory_limit_bytes": 128 * 1024 * 1024,
                    "env": {"HELIXIR_CONFIG": "/private/partial.toml"},
                }
            ],
        }
        with self.assertRaisesRegex(mb.HarnessError, "must declare llm_runtime"):
            mb.validate_trace(value)
        value["scenarios"][0]["llm_runtime"] = scenario()["llm_runtime"]
        mb.validate_trace(value)

    def test_partial_config_inherits_private_auth_and_pins_gpt_oss(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            private = Path(directory) / "helixir.toml"
            private.write_text(
                'llm_provider = "cerebras"\n'
                'llm_model = "gpt-oss-120b"\n'
                'llm_api_key = "private-test-key"\n',
                encoding="utf-8",
            )
            private.chmod(0o600)
            with patch.dict(
                os.environ,
                {"HELIXIR_MEMORY_HARNESS_LLM_CONFIG": str(private)},
                clear=False,
            ):
                expanded = mb.scenario_env(scenario(), Path(directory) / "output")
        self.assertEqual(expanded["HELIX_LLM_PROVIDER"], "cerebras")
        self.assertEqual(expanded["HELIX_LLM_MODEL"], "gpt-oss-120b")
        self.assertEqual(expanded["HELIX_LLM_API_KEY"], "private-test-key")
        self.assertNotIn("private-test-key", json.dumps(scenario()))

    def test_private_llm_runtime_rejects_missing_or_empty_auth(self) -> None:
        with (
            patch.dict(
                os.environ,
                {"HELIXIR_MEMORY_HARNESS_LLM_CONFIG": ""},
                clear=False,
            ),
            self.assertRaisesRegex(mb.HarnessError, "is required"),
        ):
            mb.scenario_env(scenario(), Path("/private/output"))
        with tempfile.TemporaryDirectory() as directory:
            private = Path(directory) / "helixir.toml"
            private.write_text(
                'llm_provider = "cerebras"\nllm_model = "gpt-oss-120b"\n',
                encoding="utf-8",
            )
            private.chmod(0o600)
            with (
                patch.dict(
                    os.environ,
                    {"HELIXIR_MEMORY_HARNESS_LLM_CONFIG": str(private)},
                    clear=False,
                ),
                self.assertRaisesRegex(mb.HarnessError, "non-empty llm_api_key"),
            ):
                mb.scenario_env(scenario(), Path(directory) / "output")


if __name__ == "__main__":
    unittest.main()
