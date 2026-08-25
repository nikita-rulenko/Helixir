#!/usr/bin/env python3
"""Reset or wait for one disposable HelixDB profiling target."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path
from urllib.parse import urlparse


class TargetError(RuntimeError):
    """The disposable target violated its safety contract."""


def run(argv: list[str], *, check: bool = True) -> str:
    result = subprocess.run(argv, text=True, capture_output=True)
    if check and result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise TargetError(f"command failed ({result.returncode}): {argv!r}: {detail}")
    return result.stdout.strip()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            digest.update(block)
    return digest.hexdigest()


def validate_daemon(expected_id: str) -> None:
    parsed = urlparse(os.environ.get("DOCKER_HOST", ""))
    if parsed.scheme != "tcp" or parsed.hostname not in {"127.0.0.1", "localhost"}:
        raise TargetError("DOCKER_HOST must be a loopback TCP disposable daemon")
    actual = run(["docker", "info", "--format", "{{.ID}}"])
    if actual != expected_id:
        raise TargetError(f"disposable daemon mismatch: expected {expected_id}, got {actual}")


def safe_name(value: str, kind: str) -> None:
    if not re.fullmatch(r"helixir-profile-[a-z0-9][a-z0-9-]*", value):
        raise TargetError(f"unsafe {kind} name: {value}")


def reset(args: argparse.Namespace) -> None:
    validate_daemon(args.daemon_id)
    safe_name(args.container, "container")
    safe_name(args.volume, "volume")
    if args.port == 6970 or not 1024 <= args.port <= 65535:
        raise TargetError("profile port is invalid or aliases production 6970")
    archive = args.archive.expanduser().resolve()
    if not archive.is_file() or sha256(archive) != args.archive_sha256:
        raise TargetError("cold archive is missing or its SHA-256 changed")
    actual_image = run(["docker", "image", "inspect", "-f", "{{.Id}}", args.image])
    if actual_image != args.image_id:
        raise TargetError(f"image mismatch: expected {args.image_id}, got {actual_image}")

    helper = f"{args.container}-restore"
    run(["docker", "rm", "-f", args.container], check=False)
    run(["docker", "rm", "-f", helper], check=False)
    run(["docker", "volume", "rm", args.volume], check=False)
    run(["docker", "volume", "create", "--name", args.volume])
    try:
        run(
            [
                "docker",
                "create",
                "--name",
                helper,
                "-v",
                f"{args.volume}:/data",
                args.helper_image,
                "sleep",
                "300",
            ]
        )
        run(["docker", "start", helper])
        run(["docker", "cp", str(archive), f"{helper}:/tmp/cold.tar.gz"])
        run(
            [
                "docker",
                "exec",
                helper,
                "tar",
                "-xzf",
                "/tmp/cold.tar.gz",
                "-C",
                "/data",
                f"--strip-components={args.strip_components}",
            ]
        )
    finally:
        run(["docker", "rm", "-f", helper], check=False)

    run(
        [
            "docker",
            "run",
            "--detach",
            "--name",
            args.container,
            "--memory",
            str(args.memory_bytes),
            "--memory-swap",
            str(args.memory_bytes),
            "--cpus",
            str(args.cpus),
            "--publish",
            f"{args.port}:6969",
            "--volume",
            f"{args.volume}:/data",
            "--env",
            "HELIX_CORES_OVERRIDE=1",
            "--env",
            "HELIX_DATA_DIR=/data",
            "--env",
            "HELIX_INSTANCE=profile",
            "--env",
            "HELIX_PORT=6969",
            "--env",
            "HELIX_PROJECT=helixir-profile",
            "--env",
            "MIMALLOC_ARENA_PURGE_MULT=1",
            "--env",
            "MIMALLOC_PURGE_DECOMMITS=1",
            "--env",
            "MIMALLOC_PURGE_DELAY=0",
            args.image,
        ]
    )


def wait_ready(args: argparse.Namespace) -> None:
    validate_daemon(args.daemon_id)
    payload = b"{}"
    deadline = time.monotonic() + args.timeout
    last_error = "not attempted"
    while time.monotonic() < deadline:
        request = urllib.request.Request(
            args.url,
            data=payload,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urllib.request.urlopen(request, timeout=3) as response:
                if response.status == 200:
                    json.loads(response.read())
                    return
                last_error = f"HTTP {response.status}"
        except (OSError, ValueError, urllib.error.URLError) as error:
            last_error = str(error)
        time.sleep(0.5)
    raise TargetError(f"HelixDB did not become ready: {last_error}")


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    commands = root.add_subparsers(dest="command", required=True)
    reset_parser = commands.add_parser("reset")
    reset_parser.add_argument("--daemon-id", required=True)
    reset_parser.add_argument("--container", required=True)
    reset_parser.add_argument("--volume", required=True)
    reset_parser.add_argument("--image", required=True)
    reset_parser.add_argument("--image-id", required=True)
    reset_parser.add_argument("--archive", type=Path, required=True)
    reset_parser.add_argument("--archive-sha256", required=True)
    reset_parser.add_argument("--port", type=int, required=True)
    reset_parser.add_argument("--memory-bytes", type=int, default=3 * 1024**3)
    reset_parser.add_argument("--cpus", type=float, default=1.0)
    reset_parser.add_argument("--strip-components", type=int, default=1)
    reset_parser.add_argument("--helper-image", default="alpine:3.22")
    reset_parser.set_defaults(handler=reset)
    wait_parser = commands.add_parser("wait")
    wait_parser.add_argument("--daemon-id", required=True)
    wait_parser.add_argument("--url", required=True)
    wait_parser.add_argument("--timeout", type=float, default=45.0)
    wait_parser.set_defaults(handler=wait_ready)
    return root


def main() -> int:
    try:
        args = parser().parse_args()
        args.handler(args)
        return 0
    except (TargetError, OSError, ValueError, json.JSONDecodeError) as error:
        print(f"helixdb-profile-target: {error}", file=os.sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
