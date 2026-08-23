#!/usr/bin/env python3
"""Deterministic, Docker-free checks for the canonical live E2E matrix."""

import os
import unittest
from unittest import mock

import e2e_matrix


class E2eMatrixTests(unittest.TestCase):
    def test_manifest_exactly_covers_discovered_ignored_tests(self) -> None:
        rows = e2e_matrix.validate_manifest(e2e_matrix.load_manifest())
        self.assertEqual(len(rows), 59)
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

    def test_production_port_is_rejected_even_with_disposable_opt_in(self) -> None:
        with mock.patch.dict(os.environ, {"HELIXIR_E2E_DISPOSABLE": "1"}, clear=False):
            with self.assertRaisesRegex(e2e_matrix.ManifestError, "production"):
                e2e_matrix.ensure_disposable_target("127.0.0.1", 6970)


if __name__ == "__main__":
    unittest.main()
