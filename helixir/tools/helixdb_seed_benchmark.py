#!/usr/bin/env python3
"""
Populate HelixDB with multi-user benchmark data over HTTP (/queryName).

Creates users, memories (varied text for BM25), embeddings (768-d unit vectors),
reasoning edges (IMPLIES / BECAUSE / CONTRADICTS / MEMORY_RELATION).

Requires: Python 3.9+ (stdlib only). Default embedding dim matches data-model (768).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import random
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from typing import Any

TOPICS = [
    ("Rust async и производительность сервисов", "fact", "systems rust"),
    ("Предпочитаю олламу для локальных эмбеддингов", "preference", "ollama local"),
    ("Цель: сравнить BM25 и векторный поиск на графе", "goal", "benchmark search"),
    ("Мнение: RRF стабилизирует смешанный ретривал", "opinion", "rrf hybrid"),
    ("Опыт деплоя HelixDB через Ansible на VM", "experience", "ansible helixdb"),
    ("PostgreSQL индексы GIN для полнотекста хуже чем специализированный BM25", "fact", "postgres bm25"),
    ("Навык настройки vector_config m ef_construction ef_search", "skill", "hnsw tuning"),
    ("Достижение: поднял инстанс с shared memory между пользователями", "achievement", "shared memory"),
    ("Действие: архивирую старую db перед миграцией", "action", "backup migrate"),
]


def _now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def unit_vector(seed: str, dim: int) -> list[float]:
    h = int(hashlib.sha256(seed.encode()).hexdigest()[:8], 16)
    rng = random.Random(h)
    raw = [rng.gauss(0.0, 1.0) for _ in range(dim)]
    norm = math.sqrt(sum(x * x for x in raw)) or 1.0
    return [x / norm for x in raw]


def post_json(base: str, query_name: str, payload: dict[str, Any], timeout: int = 120) -> dict[str, Any]:
    url = f"{base.rstrip('/')}/{query_name}"
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            body = resp.read().decode("utf-8")
            if not body:
                return {}
            return json.loads(body)
    except urllib.error.HTTPError as e:
        detail = e.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"HTTP {e.code} on {query_name}: {detail}") from e


def extract_memory_internal_id(response: dict[str, Any]) -> str | None:
    if not response:
        return None
    mem = response.get("memory")
    if isinstance(mem, dict):
        for key in ("id", "ID", "Id"):
            if key in mem and mem[key] is not None:
                return str(mem[key])
    # Some responses nest differently
    for k, v in response.items():
        if isinstance(v, dict) and "id" in v and k.lower() == "memory":
            return str(v["id"])
    return None


def run_seed(
    base: str,
    *,
    num_users: int,
    memories_per_user: int,
    embedding_dim: int,
    embedding_model: str,
) -> None:
    fixed_ts = _now_iso()
    all_external_ids: list[str] = []

    for ui in range(num_users):
        uid = f"bench_user_{ui:02d}"
        post_json(
            base,
            "addUser",
            {"user_id": uid, "name": f"Bench user {ui}"},
        )

        for mi in range(memories_per_user):
            topic_idx = (ui * memories_per_user + mi) % len(TOPICS)
            text, mtype, tags = TOPICS[topic_idx]
            content = f"{text} [user={uid} idx={mi}] теги:{tags} batch=bench2026"
            mem_ext = f"mem_bench_{ui:02d}_{mi:03d}"
            all_external_ids.append(mem_ext)

            r_add = post_json(
                base,
                "addMemory",
                {
                    "memory_id": mem_ext,
                    "user_id": uid,
                    "content": content,
                    "memory_type": mtype,
                    "certainty": 70 + (ui + mi) % 25,
                    "importance": 40 + (ui * 3 + mi) % 50,
                    "created_at": fixed_ts,
                    "updated_at": fixed_ts,
                    "context_tags": tags,
                    "source": "helixdb_seed_benchmark",
                    "metadata": "{}",
                },
            )

            internal = extract_memory_internal_id(r_add)
            if not internal:
                r_get = post_json(base, "getMemory", {"memory_id": mem_ext})
                internal = extract_memory_internal_id(r_get)
            if not internal:
                raise RuntimeError(f"Could not resolve internal id for {mem_ext}: {r_add!r}")

            post_json(
                base,
                "linkUserToMemory",
                {"user_id": uid, "memory_id": mem_ext, "context": "bench_seed"},
            )

            vec = unit_vector(mem_ext, embedding_dim)
            post_json(
                base,
                "addMemoryEmbedding",
                {
                    "memory_id": internal,
                    "vector_data": vec,
                    "embedding_model": embedding_model,
                    "created_at": fixed_ts,
                },
            )

    # Graph edges (external memory_id strings)
    for ui in range(num_users):
        a = f"mem_bench_{ui:02d}_000"
        b = f"mem_bench_{ui:02d}_001"
        c = f"mem_bench_{ui:02d}_002"
        try:
            post_json(
                base,
                "addMemoryImplication",
                {
                    "from_id": a,
                    "to_id": b,
                    "probability": 80,
                    "reasoning_id": f"reas_impl_{ui}",
                },
            )
        except RuntimeError:
            pass
        try:
            post_json(
                base,
                "addMemoryCausation",
                {
                    "from_id": b,
                    "to_id": c,
                    "strength": 75,
                    "reasoning_id": f"reas_cause_{ui}",
                },
            )
        except RuntimeError:
            pass

    u0, u1 = 0, min(1, num_users - 1)
    try:
        post_json(
            base,
            "addMemoryContradiction",
            {
                "from_id": f"mem_bench_{u0:02d}_003",
                "to_id": f"mem_bench_{u1:02d}_003",
                "resolution": "разные пользователи разные мнения",
                "resolved": 0,
                "resolution_strategy": "bench_cross_user",
            },
        )
    except RuntimeError:
        pass

    try:
        post_json(
            base,
            "addReasoningRelation",
            {
                "relation_id": "rel_bench_main",
                "from_memory_id": f"mem_bench_{u0:02d}_004",
                "to_memory_id": f"mem_bench_{u1:02d}_004",
                "relation_type": "semantic_link",
                "strength": 60,
                "confidence": 70,
                "explanation": "benchmark relation",
                "created_by": "helixdb_seed_benchmark",
                "created_at": fixed_ts,
            },
        )
    except RuntimeError:
        pass

    counts = post_json(base, "countAllMemories", {})
    print(
        json.dumps(
            {
                "ok": True,
                "users": num_users,
                "memories": len(all_external_ids),
                "countAllMemories_response": counts,
            },
            ensure_ascii=False,
        )
    )


def main() -> int:
    p = argparse.ArgumentParser(description="Seed HelixDB for benchmarks (HTTP).")
    p.add_argument("--base-url", default="http://127.0.0.1:6969", help="HelixDB base URL")
    p.add_argument("--users", type=int, default=12)
    p.add_argument("--memories-per-user", type=int, default=10)
    p.add_argument("--embedding-dim", type=int, default=768)
    p.add_argument("--embedding-model", default="bench-static-768")
    args = p.parse_args()

    t0 = time.perf_counter()
    run_seed(
        args.base_url,
        num_users=args.users,
        memories_per_user=args.memories_per_user,
        embedding_dim=args.embedding_dim,
        embedding_model=args.embedding_model,
    )
    dt = time.perf_counter() - t0
    print(f"seed_wall_s={dt:.3f}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
