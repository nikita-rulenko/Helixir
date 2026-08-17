#!/usr/bin/env python3
"""Bounded authenticated soak for the live control-plane read surface."""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import time
import urllib.request


def parse_mib(value: str) -> float:
    match = re.fullmatch(r"\s*([0-9.]+)\s*([KMG]iB)\s*", value)
    if not match:
        raise ValueError(f"unrecognized Docker memory value: {value!r}")
    amount = float(match.group(1))
    unit = match.group(2)
    if unit.startswith("KiB"):
        return amount / 1024
    return amount * 1024 if unit.startswith("GiB") else amount


def container_mib(name: str) -> float:
    output = subprocess.check_output(
        ["docker", "stats", "--no-stream", "--format", "{{.MemUsage}}", name],
        text=True,
    )
    return parse_mib(output.split("/")[0])


def fetch(base_url: str, token: str, path: str) -> None:
    request = urllib.request.Request(
        f"{base_url}/api/v1{path}",
        headers={"Authorization": f"Bearer {token}"},
    )
    with urllib.request.urlopen(request, timeout=20) as response:
        if response.status != 200:
            raise RuntimeError(f"{path} returned {response.status}")
        json.load(response)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", default="http://127.0.0.1:6971")
    parser.add_argument("--token-file", default="~/.helixir/run/control-plane-browser.token")
    parser.add_argument("--container", default="helixir-control-plane")
    parser.add_argument("--cycles", type=int, default=60)
    parser.add_argument("--interval", type=float, default=1.0)
    parser.add_argument("--max-growth-mib", type=float, default=96.0)
    args = parser.parse_args()
    token = pathlib.Path(args.token_file).expanduser().read_text().strip()
    if len(token) < 64:
        raise SystemExit("invalid browser token")
    paths = ["/overview", "/access", "/memory-field?page=1", "/moirai", "/health"]
    baseline = container_mib(args.container)
    peak = baseline
    started = time.monotonic()
    for _ in range(args.cycles):
        for path in paths:
            fetch(args.url, token, path)
        peak = max(peak, container_mib(args.container))
        time.sleep(args.interval)
    final = container_mib(args.container)
    growth = max(peak, final) - baseline
    if growth > args.max_growth_mib:
        raise SystemExit(
            f"FAIL: control-plane memory grew {growth:.1f} MiB "
            f"(budget {args.max_growth_mib:.1f} MiB)"
        )
    print(
        f"PASS: {args.cycles * len(paths)} authenticated reads in "
        f"{time.monotonic() - started:.1f}s; "
        f"memory {baseline:.1f} -> {final:.1f} MiB, peak {peak:.1f} MiB"
    )


if __name__ == "__main__":
    main()
