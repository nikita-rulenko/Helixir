#!/usr/bin/env python3
"""Write a concise, value-free summary of a memory-boundary report."""

from __future__ import annotations

import argparse
import json
import os
from collections import Counter
from pathlib import Path


def query_summary(path: Path | None) -> dict:
    if path is None or not path.exists():
        return {"recorded": False}
    counts: Counter[str] = Counter()
    errors: Counter[str] = Counter()
    total_micros: Counter[str] = Counter()
    with path.open(encoding="utf-8") as stream:
        for line in stream:
            row = json.loads(line)
            query = row["query"]
            counts[query] += 1
            total_micros[query] += int(row.get("duration_micros", 0))
            if row.get("status") != "ok":
                errors[query] += 1
    busiest = [
        {
            "query": query,
            "calls": count,
            "errors": errors[query],
            "total_micros": total_micros[query],
        }
        for query, count in counts.most_common(20)
    ]
    return {"recorded": True, "calls": sum(counts.values()), "busiest": busiest}


def memory_summary(result: dict) -> dict:
    samples = result.get("samples", [])
    database = [row.get("database") or {} for row in samples]
    first = database[0] if database else {}
    last = database[-1] if database else {}
    return {
        "start_bytes": first.get("current"),
        "last_bytes": last.get("current"),
        "peak_bytes": max((row.get("peak", 0) for row in database), default=0),
        "last_anon_bytes": last.get("anon"),
        "last_file_bytes": last.get("file"),
        "last_swap_bytes": last.get("swap"),
    }


def write_private(path: Path, value: dict) -> None:
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as stream:
        json.dump(value, stream, indent=2)
        stream.write("\n")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--query-trace", type=Path)
    args = parser.parse_args()
    report = json.loads(args.report.read_text(encoding="utf-8"))
    rows = []
    for result in report.get("results", []):
        rows.append(
            {
                "scenario": result["name"],
                "verdict": result["verdict"],
                "aborted": result.get("aborted"),
                "exit_code": result.get("exit_code"),
                "memory": memory_summary(result),
            }
        )
    write_private(
        args.output,
        {
            "trace_digest": report.get("trace_digest"),
            "canonical_verdict": report.get("canonical_verdict"),
            "results": rows,
            "queries": query_summary(args.query_trace),
        },
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
