#!/usr/bin/env python3
"""
Measure HelixDB HTTP query latency (vector + BM25 + graph reads + insert path).

Compare runs after changing helix.toml (e.g. BM25 / HNSW). Requires a seeded DB
(see helixdb_seed_benchmark.py).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import random
import statistics
import sys
import time
import urllib.error
import urllib.request
from typing import Any


def unit_vector(seed: str, dim: int) -> list[float]:
    h = int(hashlib.sha256(seed.encode()).hexdigest()[:8], 16)
    rng = random.Random(h)
    raw = [rng.gauss(0.0, 1.0) for _ in range(dim)]
    norm = math.sqrt(sum(x * x for x in raw)) or 1.0
    return [x / norm for x in raw]


def post_json(base: str, query_name: str, payload: dict[str, Any], timeout: int = 60) -> dict[str, Any]:
    url = f"{base.rstrip('/')}/{query_name}"
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        body = resp.read().decode("utf-8")
        if not body:
            return {}
        return json.loads(body)


def percentile(sorted_vals: list[float], p: float) -> float:
    if not sorted_vals:
        return 0.0
    k = (len(sorted_vals) - 1) * p / 100.0
    f = int(math.floor(k))
    c = int(math.ceil(k))
    if f == c:
        return sorted_vals[f]
    return sorted_vals[f] + (sorted_vals[c] - sorted_vals[f]) * (k - f)


def bench_loop(
    name: str,
    fn: Any,
    warmup: int,
    iterations: int,
) -> dict[str, Any]:
    lat: list[float] = []
    for i in range(warmup + iterations):
        t0 = time.perf_counter()
        try:
            fn()
        except urllib.error.HTTPError as e:
            err = e.read().decode("utf-8", errors="replace")
            print(f"FAIL {name}: HTTP {e.code} {err}", file=sys.stderr)
            return {"error": f"http_{e.code}", "detail": err[:500]}
        except Exception as e:
            print(f"FAIL {name}: {e}", file=sys.stderr)
            return {"error": str(e)}
        dt_ms = (time.perf_counter() - t0) * 1000.0
        if i >= warmup:
            lat.append(dt_ms)
    s = sorted(lat)
    return {
        "n": len(lat),
        "mean_ms": round(statistics.mean(lat), 3) if lat else 0,
        "p50_ms": round(percentile(s, 50), 3),
        "p95_ms": round(percentile(s, 95), 3),
    }


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--base-url", default="http://127.0.0.1:6969")
    p.add_argument("--iterations", type=int, default=50)
    p.add_argument("--warmup", type=int, default=5)
    p.add_argument("--embedding-dim", type=int, default=768)
    p.add_argument("--probe-user", default="bench_user_00")
    p.add_argument("--probe-memory", default="mem_bench_00_000")
    p.add_argument("--cutoff-date", default="1970-01-01T00:00:00Z")
    args = p.parse_args()

    base = args.base_url
    vec = unit_vector("bench-query", args.embedding_dim)
    fixed_ts = "2026-05-12T12:00:00Z"

    add_seq = [0]

    def add_memory_once() -> None:
        add_seq[0] += 1
        mem_ext = f"mem_bench_add_{add_seq[0]}_{time.perf_counter_ns()}"
        post_json(
            base,
            "addMemory",
            {
                "memory_id": mem_ext,
                "user_id": args.probe_user,
                "content": "Ephemeral row for add-path latency measurement",
                "memory_type": "fact",
                "certainty": 50,
                "importance": 30,
                "created_at": fixed_ts,
                "updated_at": fixed_ts,
                "context_tags": "bench",
                "source": "helixdb_http_bench",
                "metadata": "{}",
            },
        )

    cases: list[tuple[str, Any]] = [
        (
            "searchMemoriesByBm25",
            lambda: post_json(
                base,
                "searchMemoriesByBm25",
                {"text": "Ansible HelixDB BM25 бенчмарк hybrid", "limit": 20},
            ),
        ),
        (
            "smartVectorSearchWithChunksCutoff",
            lambda: post_json(
                base,
                "smartVectorSearchWithChunksCutoff",
                {
                    "query_vector": vec,
                    "limit": 20,
                    "cutoff_date": args.cutoff_date,
                },
            ),
        ),
        (
            "vectorSearch",
            lambda: post_json(
                base,
                "vectorSearch",
                {
                    "query_vector": vec,
                    "user_id": args.probe_user,
                    "limit": 20,
                    "min_score": 0.0,
                },
            ),
        ),
        (
            "getMemoryLogicalConnections",
            lambda: post_json(
                base,
                "getMemoryLogicalConnections",
                {"memory_id": args.probe_memory},
            ),
        ),
        (
            "getMemoryGraphStats",
            lambda: post_json(
                base,
                "getMemoryGraphStats",
                {"memory_id": args.probe_memory},
            ),
        ),
        ("addMemory", add_memory_once),
    ]

    report: dict[str, Any] = {
        "base_url": base,
        "iterations": args.iterations,
        "warmup": args.warmup,
        "embedding_dim": args.embedding_dim,
        "cases": {},
    }

    for name, fn in cases:
        report["cases"][name] = bench_loop(name, fn, args.warmup, args.iterations)

    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
