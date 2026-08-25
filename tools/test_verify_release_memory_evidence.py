from __future__ import annotations

import copy
import datetime as dt
import json
import tempfile
import unittest
from pathlib import Path

from tools import verify_release_memory_evidence as verifier


SHA_A = "a" * 64
SHA_B = "b" * 64
SHA_C = "c" * 64
SHA_D = "d" * 64
IMAGE = "sha256:" + "e" * 64
NOW = dt.datetime(2026, 8, 25, 12, 0, tzinfo=dt.timezone.utc)


class ReleaseMemoryEvidenceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / "runtime").mkdir()
        (self.root / "runtime/a.rs").write_text("fn a() {}\n", encoding="utf-8")
        digest, count = verifier.runtime_source_fingerprint(self.root, ["runtime"])
        self.evidence = {
            "schema_version": 1,
            "release_tag": "v0.18.0",
            "observed_at": "2026-08-24T18:38:54Z",
            "max_age_days": 14,
            "evidence_lane": "faithful",
            "canonical_verdict": "pass",
            "instrumentation": "none",
            "limits_policy": {
                "max_database_bytes": 3 * verifier.GIB,
                "max_workload_bytes": verifier.GIB,
                "max_abort_fraction": 0.85,
            },
            "runtime_source": {
                "algorithm": "sha256-path-nul-content-nul-v1",
                "includes": ["runtime"],
                "sha256": digest,
                "file_count": count,
            },
            "required_scenarios": ["daemon-full-pass"],
            "repeated_scenario": "daemon-full-pass",
            "runs": [self.make_run("primary", SHA_A), self.make_run("repeat", SHA_B)],
        }
        self.path = self.root / "evidence.json"
        self.write()

    def tearDown(self) -> None:
        self.temp.cleanup()

    @staticmethod
    def make_run(name: str, trace_digest: str) -> dict:
        return {
            "id": name,
            "verdict": "pass",
            "evidence_lane": "faithful",
            "instrumentation": "none",
            "build_profile": "profiling",
            "allocator": "mimalloc",
            "tested_git_commit": "1" * 40,
            "private_report_sha256": SHA_C if name == "primary" else SHA_D,
            "trace_digest": trace_digest,
            "tested_candidate_source_sha256": SHA_A,
            "candidate_binary_sha256": SHA_B,
            "backend_image_id": IMAGE,
            "cold_state_sha256": SHA_C,
            "scenarios": [
                {
                    "name": "daemon-full-pass",
                    "verdict": "pass",
                    "exit_code": 0,
                    "aborted": False,
                    "database_limit_bytes": 3 * verifier.GIB,
                    "workload_limit_bytes": verifier.GIB,
                    "abort_fraction": 0.85,
                    "database_peak_bytes": 128 * 1024**2,
                    "workload_peak_bytes": 700 * 1024**2,
                    "oom_events": 0,
                    "oom_kill_events": 0,
                    "restarts": 0,
                    "clean_restart": True,
                    "separate_measurements": True,
                    "cold_checksum_before": SHA_C,
                    "cold_checksum_after": SHA_C,
                    "backend_image_before": IMAGE,
                    "backend_image_after": IMAGE,
                }
            ],
        }

    def write(self) -> None:
        self.path.write_text(json.dumps(self.evidence), encoding="utf-8")

    def assert_rejected(self, pattern: str) -> None:
        self.write()
        with self.assertRaisesRegex(verifier.EvidenceError, pattern):
            verifier.verify_evidence(self.path, self.root, "v0.18.0", now=NOW)

    def test_accepts_two_faithful_runs_bound_to_runtime_source(self) -> None:
        result = verifier.verify_evidence(self.path, self.root, "v0.18.0", now=NOW)
        self.assertEqual(result["faithful_runs"], 2)
        self.assertEqual(result["verified_scenarios"], 2)

    def test_missing_evidence_fails_closed(self) -> None:
        with self.assertRaisesRegex(verifier.EvidenceError, "missing"):
            verifier.verify_evidence(
                self.root / "absent.json", self.root, "v0.18.0", now=NOW
            )

    def test_tag_mismatch_fails_closed(self) -> None:
        with self.assertRaisesRegex(verifier.EvidenceError, "requested release tag"):
            verifier.verify_evidence(self.path, self.root, "v0.18.1", now=NOW)

    def test_stale_evidence_fails_closed(self) -> None:
        self.evidence["observed_at"] = "2026-07-01T00:00:00Z"
        self.assert_rejected("stale")

    def test_runtime_source_drift_fails_closed(self) -> None:
        (self.root / "runtime/a.rs").write_text("fn changed() {}\n", encoding="utf-8")
        with self.assertRaisesRegex(verifier.EvidenceError, "fingerprint drifted"):
            verifier.verify_evidence(self.path, self.root, "v0.18.0", now=NOW)

    def test_failed_verdict_fails_closed(self) -> None:
        self.evidence["canonical_verdict"] = "fail"
        self.assert_rejected("verdict is not pass")

    def test_limit_or_abort_weakening_fails_closed(self) -> None:
        scenario = self.evidence["runs"][0]["scenarios"][0]
        for field, value, pattern in (
            ("database_limit_bytes", 3 * verifier.GIB + 1, "3 GiB"),
            ("workload_limit_bytes", verifier.GIB + 1, "1 GiB"),
            ("abort_fraction", 0.86, "0.85"),
        ):
            with self.subTest(field=field):
                prior = scenario[field]
                scenario[field] = value
                self.assert_rejected(pattern)
                scenario[field] = prior

    def test_abort_oom_and_restart_each_fail_closed(self) -> None:
        scenario = self.evidence["runs"][0]["scenarios"][0]
        for field, value, pattern in (
            ("aborted", "database_memory_guard", "abort guard"),
            ("oom_events", 1, "OOM event"),
            ("oom_kill_events", 1, "OOM event"),
            ("restarts", 1, "cleanly restart"),
            ("clean_restart", False, "cleanly restart"),
        ):
            with self.subTest(field=field):
                prior = scenario[field]
                scenario[field] = value
                self.assert_rejected(pattern)
                scenario[field] = prior

    def test_cold_and_image_checksum_drift_fail_closed(self) -> None:
        scenario = self.evidence["runs"][0]["scenarios"][0]
        scenario["cold_checksum_after"] = SHA_D
        self.assert_rejected("cold-state checksum drifted")
        scenario["cold_checksum_after"] = SHA_C
        scenario["backend_image_after"] = "sha256:" + "f" * 64
        self.assert_rejected("backend image checksum drifted")

    def test_repeat_must_use_same_candidate_and_distinct_trace(self) -> None:
        self.evidence["runs"][1]["candidate_binary_sha256"] = SHA_D
        self.assert_rejected("repeat run checksum drifted")
        self.evidence["runs"][1]["candidate_binary_sha256"] = SHA_B
        self.evidence["runs"][1]["trace_digest"] = SHA_A
        self.assert_rejected("checksums are duplicated")

    def test_private_paths_and_raw_fields_are_rejected(self) -> None:
        for mutation in (
            {"candidate_binary_path": "/private/bin"},
            {"note": "/Users/operator/private"},
            {"request_body": "redacted"},
        ):
            with self.subTest(mutation=mutation):
                original = copy.deepcopy(self.evidence)
                self.evidence.update(mutation)
                self.assert_rejected("forbidden|private path")
                self.evidence = original


if __name__ == "__main__":
    unittest.main()
