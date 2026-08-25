#!/usr/bin/env python3
"""Build and inject the immutable managed-HelixDB release descriptor."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import re
import tarfile
import tempfile
from pathlib import Path

SERVER_ARCHIVE = re.compile(r"^helixir-(?:linux|macos|windows)-.+\.tar\.gz$")
HEX_40 = re.compile(r"^[0-9a-f]{40}$")
HEX_64 = re.compile(r"^[0-9a-f]{64}$")


def schema_fingerprint(schema_dir: Path) -> str:
    digest = hashlib.sha256()
    for name in ("schema.hx", "queries.hx"):
        digest.update(name.encode())
        digest.update(b"\0")
        digest.update((schema_dir / name).read_bytes())
        digest.update(b"\0")
    return f"sha256:{digest.hexdigest()}"


def rust_constant(source: Path, name: str) -> str:
    pattern = re.compile(rf'^pub const {re.escape(name)}: &str = "([^"]+)";', re.M)
    match = pattern.search(source.read_text(encoding="utf-8"))
    if not match:
        raise ValueError(f"missing Rust constant {name}")
    return match.group(1)


def validate_hex(value: str, pattern: re.Pattern[str], label: str) -> None:
    if not pattern.fullmatch(value):
        raise ValueError(f"invalid {label}")


def descriptor(args: argparse.Namespace) -> dict[str, object]:
    validate_hex(args.image_digest, HEX_64, "image digest")
    validate_hex(args.source_sha256, HEX_64, "source checksum")
    validate_hex(args.fork_revision, HEX_40, "fork revision")
    backend_rs = args.repo_root / "helixir/src/installer/backend.rs"
    return {
        "format_version": 1,
        "image": f"{args.image_repository}@sha256:{args.image_digest}",
        "engine_revision": rust_constant(backend_rs, "ENGINE_REVISION"),
        "schema_fingerprint": schema_fingerprint(args.repo_root / "helixir/schema"),
        "source_url": args.source_url,
        "source_sha256": args.source_sha256,
        "upstream_revision": rust_constant(backend_rs, "UPSTREAM_REVISION"),
        "fork_revision": args.fork_revision,
        "license": "AGPL-3.0-only",
    }


def safe_extract(archive: Path, destination: Path) -> None:
    with tarfile.open(archive, "r:gz") as bundle:
        root = destination.resolve()
        for member in bundle.getmembers():
            target = (destination / member.name).resolve()
            if root != target and root not in target.parents:
                raise ValueError(f"unsafe archive member {member.name}")
        if hasattr(tarfile, "data_filter"):
            bundle.extractall(destination, filter="data")
        else:
            bundle.extractall(destination)


def deterministic_archive(source: Path, destination: Path) -> None:
    temporary = destination.with_suffix(destination.suffix + ".tmp")
    with temporary.open("wb") as output, gzip.GzipFile(
        fileobj=output, mode="wb", mtime=0
    ) as compressed, tarfile.open(fileobj=compressed, mode="w") as bundle:
        for path in sorted(source.rglob("*")):
            relative = path.relative_to(source)
            info = bundle.gettarinfo(str(path), arcname=str(relative))
            info.uid = info.gid = 0
            info.uname = info.gname = "root"
            info.mtime = 0
            if path.is_file():
                with path.open("rb") as payload:
                    bundle.addfile(info, payload)
            else:
                bundle.addfile(info)
    temporary.replace(destination)


def inject(args: argparse.Namespace) -> int:
    payload = descriptor(args)
    encoded = json.dumps(payload, indent=2, sort_keys=True).encode() + b"\n"
    archives = sorted(
        path
        for path in args.artifacts_root.rglob("*.tar.gz")
        if SERVER_ARCHIVE.fullmatch(path.name)
    )
    if not archives:
        raise ValueError("no server release archives found")
    for archive in archives:
        with tempfile.TemporaryDirectory(prefix="helixir-backend-release-") as work:
            root = Path(work)
            safe_extract(archive, root)
            (root / "backend-image.json").write_bytes(encoded)
            deterministic_archive(root, archive)
    for path in args.artifacts_root.rglob("helixir-client-*.tar.gz"):
        with tarfile.open(path, "r:gz") as bundle:
            if any(Path(member.name).name == "backend-image.json" for member in bundle):
                raise ValueError(f"thin client archive contains backend descriptor: {path}")
    if args.output:
        args.output.write_bytes(encoded)
    return len(archives)


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser()
    sub = result.add_subparsers(dest="command", required=True)
    fingerprint = sub.add_parser("fingerprint")
    fingerprint.add_argument("--schema-dir", type=Path, required=True)
    inject_parser = sub.add_parser("inject")
    inject_parser.add_argument("--repo-root", type=Path, required=True)
    inject_parser.add_argument("--artifacts-root", type=Path, required=True)
    inject_parser.add_argument("--image-repository", required=True)
    inject_parser.add_argument("--image-digest", required=True)
    inject_parser.add_argument("--source-url", required=True)
    inject_parser.add_argument("--source-sha256", required=True)
    inject_parser.add_argument("--fork-revision", required=True)
    inject_parser.add_argument("--output", type=Path)
    return result


def main() -> None:
    args = parser().parse_args()
    if args.command == "fingerprint":
        print(schema_fingerprint(args.schema_dir))
    else:
        print(inject(args))


if __name__ == "__main__":
    main()
