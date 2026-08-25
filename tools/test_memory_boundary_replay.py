#!/usr/bin/env python3
"""Replay and dry-run tests for the memory-boundary harness."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from copy import deepcopy
from pathlib import Path

from tools import memory_boundary_lib as mb
from tools.test_memory_boundary import trace


class ReplayAndDryRunTests(unittest.TestCase):
    def test_trace_digest_is_canonical(self) -> None:
        left = trace()
        right = deepcopy(left)
        right["sampling"] = dict(reversed(list(right["sampling"].items())))
        self.assertEqual(mb.trace_digest(left), mb.trace_digest(right))

    def test_private_metadata_is_confined_and_mode_0600(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            artifact = output / "heap.dump"
            artifact.write_bytes(b"private")
            artifact.chmod(0o640)
            with self.assertRaisesRegex(mb.HarnessError, "confined"):
                mb.private_metadata(output, ["heap.dump"])
            with self.assertRaisesRegex(mb.HarnessError, "relative to output_dir"):
                mb.private_metadata(output, ["/tmp/outside.dump"])

    def test_dry_run_and_identical_replay_never_contact_docker(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            trace_path = root / "trace.json"
            trace_path.write_text(json.dumps(trace()), encoding="utf-8")
            script = Path(__file__).with_name("memory_boundary.py")
            env = dict(os.environ)
            env.pop("HELIXIR_MEMORY_HARNESS_DISPOSABLE_DOCKER", None)
            first = subprocess.run(
                [sys.executable, str(script), "--trace", str(trace_path), "--dry-run"],
                text=True,
                capture_output=True,
                env=env,
            )
            self.assertEqual(first.returncode, 0, first.stderr)
            digest = json.loads(first.stdout)["trace_digest"]
            report_path = root / "report.json"
            report_path.write_text(
                json.dumps({"trace": trace(), "trace_digest": digest}), encoding="utf-8"
            )
            replay = subprocess.run(
                [
                    sys.executable,
                    str(script),
                    "--replay",
                    str(report_path),
                    "--dry-run",
                ],
                text=True,
                capture_output=True,
                env=env,
            )
            self.assertEqual(replay.returncode, 0, replay.stderr)
            metadata = json.loads(replay.stdout)["run_metadata"]
            self.assertEqual(metadata["run_id"], "unit-run")
            self.assertEqual(metadata["allocator"], "system")
            self.assertEqual(metadata["profilers"], [])

    def test_process_rss_is_sampled_without_model_or_docker(self) -> None:
        sample = mb.process_memory(os.getpid())
        self.assertGreater(sample["rss"], 0)
        self.assertIn("smaps_rollup", sample)


if __name__ == "__main__":
    unittest.main()
