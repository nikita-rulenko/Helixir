#!/usr/bin/env python3
"""Deterministic, Docker-free checks for the canonical live E2E matrix."""

import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import e2e_matrix


class E2eMatrixTests(unittest.TestCase):
    def test_manifest_exactly_covers_discovered_ignored_tests(self) -> None:
        rows = e2e_matrix.validate_manifest(e2e_matrix.load_manifest())
        self.assertEqual(len(rows), 60)
        self.assertEqual(
            {f"{row['target']}::{row['test']}" for row in rows},
            set(e2e_matrix.discover_ignored_tests()),
        )

    def test_ingest_environment_is_owned_per_suite(self) -> None:
        rows = e2e_matrix.validate_manifest(e2e_matrix.load_manifest())
        enabled = next(row for row in rows if row["target"] == "mcp_ingest_e2e")
        disabled = next(row for row in rows if row["target"] == "chunking_e2e")
        with mock.patch.dict(os.environ, {"HELIXIR_INGEST_BUFFER": "stale"}, clear=False):
            self.assertEqual(
                e2e_matrix.build_environment(enabled, "codex", "default")[
                    "HELIXIR_INGEST_BUFFER"
                ],
                "1",
            )
            self.assertNotIn(
                "HELIXIR_INGEST_BUFFER",
                e2e_matrix.build_environment(disabled, "codex", "default"),
            )

    def test_profiling_probe_requires_explicit_selection(self) -> None:
        rows = e2e_matrix.validate_manifest(e2e_matrix.load_manifest())
        probe = next(
            row
            for row in rows
            if row["target"] == "daemon_e2e"
            and row["test"] == "daemon_profile_stage_runs_exactly_one_pass"
        )
        self.assertIs(probe.get("run_by_default"), False)

    def test_production_port_is_rejected_even_with_disposable_opt_in(self) -> None:
        with mock.patch.dict(os.environ, {"HELIXIR_E2E_DISPOSABLE": "1"}, clear=False):
            with self.assertRaisesRegex(e2e_matrix.ManifestError, "production"):
                e2e_matrix.ensure_disposable_target("127.0.0.1", 6970)

    def test_storage_thresholds_must_be_positive_numbers(self) -> None:
        for value in ("0", "-1", "not-a-number"):
            with self.subTest(value=value):
                with mock.patch.dict(
                    os.environ,
                    {"HELIXIR_E2E_MIN_FREE_GIB": value},
                    clear=False,
                ):
                    with self.assertRaisesRegex(
                        e2e_matrix.ManifestError, "must be a positive number"
                    ):
                        e2e_matrix._positive_gib(
                            "HELIXIR_E2E_MIN_FREE_GIB", 20.0
                        )

    def test_storage_preflight_fails_before_creating_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temp_root = Path(directory)
            with mock.patch.dict(
                os.environ,
                {
                    "HELIXIR_E2E_TEMP_ROOT": str(temp_root),
                    "HELIXIR_E2E_MIN_FREE_GIB": "20",
                    "HELIXIR_E2E_MAX_TARGET_GIB": "24",
                },
                clear=False,
            ):
                with mock.patch.object(
                    e2e_matrix.shutil,
                    "disk_usage",
                    return_value=mock.Mock(free=43 * e2e_matrix.GIB),
                ):
                    with self.assertRaisesRegex(
                        e2e_matrix.ManifestError, "requires 44.00 GiB"
                    ):
                        e2e_matrix.create_bounded_cargo_target()
            self.assertEqual(list(temp_root.iterdir()), [])

    def test_storage_envelope_rejects_target_growth_and_low_headroom(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            with mock.patch.object(
                e2e_matrix, "_tree_bytes", return_value=25 * e2e_matrix.GIB
            ):
                with self.assertRaisesRegex(e2e_matrix.ManifestError, "limit is 24"):
                    e2e_matrix.check_storage_envelope(target, 20.0, 24.0)

            with mock.patch.object(e2e_matrix, "_tree_bytes", return_value=1):
                with mock.patch.object(
                    e2e_matrix.shutil,
                    "disk_usage",
                    return_value=mock.Mock(free=19 * e2e_matrix.GIB),
                ):
                    with self.assertRaisesRegex(
                        e2e_matrix.ManifestError, "safety floor is 20"
                    ):
                        e2e_matrix.check_storage_envelope(target, 20.0, 24.0)

    def test_cleanup_removes_disposable_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "target"
            target.mkdir()
            (target / "artifact").write_bytes(b"build-cache")
            e2e_matrix.cleanup_cargo_target(target)
            self.assertFalse(target.exists())


if __name__ == "__main__":
    unittest.main()
