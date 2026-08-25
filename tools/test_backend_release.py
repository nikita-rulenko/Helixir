import argparse
import hashlib
import json
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

try:
    from tools import backend_release
except ModuleNotFoundError:
    import backend_release


class BackendReleaseTests(unittest.TestCase):
    @staticmethod
    def _args(
        repo: Path, root: Path, output: Path | None = None
    ) -> argparse.Namespace:
        return argparse.Namespace(
            repo_root=repo,
            artifacts_root=root,
            image_repository="ghcr.io/example/helixir-helixdb",
            image_digest="a" * 64,
            source_url="https://example.test/source.tar.gz",
            source_sha256="b" * 64,
            fork_revision="c" * 40,
            output=output,
        )

    def test_maintained_fork_dockerfile_pins_external_base_images(self):
        repo = Path(__file__).resolve().parents[1]
        dockerfile = (repo / "helixdb/helix-db/Dockerfile").read_text(encoding="utf-8")
        external_images = {
            line.split()[1]
            for line in dockerfile.splitlines()
            if line.startswith("FROM ") and line.split()[1] not in {"chef", "planner", "builder"}
        }

        self.assertEqual(len(external_images), 2)
        for image in external_images:
            self.assertRegex(image, r"^[^@\s]+@sha256:[0-9a-f]{64}$")

    def test_cli_exposes_one_required_subcommand_group(self):
        script = Path(__file__).with_name("backend_release.py")
        result = subprocess.run(
            [sys.executable, str(script), "--help"],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("{fingerprint,inject}", result.stdout)

    def test_injects_every_server_archive_and_no_client_archive(self):
        repo = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            server_names = (
                "helixir-linux-x86_64.tar.gz",
                "helixir-linux-arm64.tar.gz",
                "helixir-macos-x86_64.tar.gz",
                "helixir-macos-arm64.tar.gz",
                "helixir-windows-x86_64.tar.gz",
            )
            client_names = (
                "helixir-client-linux-x86_64.tar.gz",
                "helixir-client-linux-arm64.tar.gz",
                "helixir-client-macos-x86_64.tar.gz",
                "helixir-client-macos-arm64.tar.gz",
                "helixir-client-windows-x86_64.tar.gz",
            )
            for name in (*server_names, *client_names):
                payload = root / name.removesuffix(".tar.gz")
                payload.mkdir()
                (payload / "binary").write_text("fixture", encoding="utf-8")
                with tarfile.open(root / name, "w:gz") as bundle:
                    bundle.add(payload / "binary", arcname="binary")
                (payload / "binary").unlink()
                payload.rmdir()
            args = self._args(repo, root, root / "backend-image.json")
            self.assertEqual(backend_release.inject(args), len(server_names))
            for name in server_names:
                with tarfile.open(root / name, "r:gz") as bundle:
                    self.assertEqual(bundle.getnames().count("backend-image.json"), 1)
                    data = json.load(bundle.extractfile("backend-image.json"))
                self.assertEqual(
                    data["image"],
                    f"ghcr.io/example/helixir-helixdb@sha256:{'a' * 64}",
                )
                self.assertEqual(data["source_sha256"], "b" * 64)
            for name in client_names:
                with tarfile.open(root / name, "r:gz") as bundle:
                    self.assertNotIn("backend-image.json", bundle.getnames())

    def test_rejects_backend_descriptor_anywhere_in_client_archive(self):
        repo = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            server = root / "server"
            server.mkdir()
            (server / "binary").write_text("fixture", encoding="utf-8")
            with tarfile.open(root / "helixir-linux-x86_64.tar.gz", "w:gz") as bundle:
                bundle.add(server / "binary", arcname="binary")

            client = root / "client"
            (client / "nested").mkdir(parents=True)
            (client / "nested/backend-image.json").write_text("{}", encoding="utf-8")
            with tarfile.open(
                root / "helixir-client-linux-x86_64.tar.gz", "w:gz"
            ) as bundle:
                bundle.add(
                    client / "nested/backend-image.json",
                    arcname="nested/backend-image.json",
                )

            with self.assertRaisesRegex(ValueError, "thin client archive"):
                backend_release.inject(self._args(repo, root))

    def test_schema_fingerprint_matches_rust_contract(self):
        repo = Path(__file__).resolve().parents[1]
        schema = repo / "helixir/schema"
        digest = hashlib.sha256()
        for name in ("schema.hx", "queries.hx"):
            digest.update(name.encode())
            digest.update(b"\0")
            digest.update((schema / name).read_bytes())
            digest.update(b"\0")
        self.assertEqual(
            backend_release.schema_fingerprint(schema), f"sha256:{digest.hexdigest()}"
        )

    def test_release_workflow_uses_valid_docker_label_templates(self):
        repo = Path(__file__).resolve().parents[1]
        workflow = (repo / ".github/workflows/release.yml").read_text(encoding="utf-8")

        for label in (
            "io.helixir.engine-revision",
            "io.helixir.schema-fingerprint",
        ):
            self.assertIn(
                f"--format '{{{{ index .Config.Labels \"{label}\" }}}}'",
                workflow,
            )
            self.assertNotIn(
                f"--format '{{{{ index .Config.Labels \\\"{label}\\\" }}}}'",
                workflow,
            )


if __name__ == "__main__":
    unittest.main()
