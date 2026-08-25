#!/usr/bin/env python3
"""Reject empty or unresolved Samply artifacts without exposing frame names."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


class SamplyValidationError(RuntimeError):
    """The profile cannot support a stack-level conclusion."""


def profile_counts(profile: dict[str, Any]) -> dict[str, int | bool]:
    threads = profile.get("threads")
    if not isinstance(threads, list):
        raise SamplyValidationError("Samply profile has no thread table")
    samples = frames = functions = 0
    for thread in threads:
        if not isinstance(thread, dict):
            continue
        samples += int((thread.get("samples") or {}).get("length", 0))
        frames += int((thread.get("frameTable") or {}).get("length", 0))
        functions += int((thread.get("funcTable") or {}).get("length", 0))
    symbolicated = bool((profile.get("meta") or {}).get("symbolicated", False))
    return {
        "threads": len(threads),
        "samples": samples,
        "frames": frames,
        "functions": functions,
        "symbolicated": symbolicated,
    }


def validate(profile: dict[str, Any]) -> dict[str, int | bool]:
    counts = profile_counts(profile)
    if counts["samples"] == 0:
        raise SamplyValidationError("Samply profile contains zero samples")
    if counts["frames"] == 0 or counts["functions"] == 0:
        raise SamplyValidationError("Samply profile contains no resolved stack frames")
    if not counts["symbolicated"]:
        raise SamplyValidationError("Samply profile is not symbolicated")
    return counts


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", required=True, type=Path)
    args = parser.parse_args()
    try:
        profile = json.loads(args.profile.read_text(encoding="utf-8"))
        counts = validate(profile)
    except (OSError, json.JSONDecodeError, SamplyValidationError) as error:
        parser.error(str(error))
    print(json.dumps(counts, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
