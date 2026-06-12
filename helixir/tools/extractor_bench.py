#!/usr/bin/env python3
"""Small-model extraction benchmark (issue #24 / local-first write path).

Compares candidate local models against a reference extraction (Cerebras
gpt-oss-120b) using the SAME compact prompt, on a fixed input set. Scoring:
fact coverage via embedding similarity (nomic-embed-text through ollama).

  recall    = share of reference facts matched by a candidate fact (cos >= 0.85)
  precision = share of candidate facts matching some reference fact
  f1        = harmonic mean; latency = wall clock per input, model warm

Usage:
  CEREBRAS_API_KEY=... python3 tools/extractor_bench.py [model ...]
Defaults: qwen2.5:1.5b qwen2.5:3b phi3
"""

import json
import os
import sys
import time
import urllib.request

OLLAMA = "http://localhost:11434"
CEREBRAS_URL = "https://api.cerebras.ai/v1/chat/completions"
REFERENCE_MODEL = "gpt-oss-120b"
MATCH_THRESHOLD = 0.85

SYSTEM = (
    "Extract atomic facts from the user's text. Reply with JSON only, no prose:\n"
    '{"facts":[{"text":"...","type":"fact|preference|skill|goal|opinion|experience|achievement|action"}]}\n'
    "Rules: one self-contained sentence per fact; preserve names, numbers, dates,"
    " identifiers exactly; do not invent facts; keep the language of the input."
)

FEW_SHOT_IN = "I deployed the API to AWS on Friday and I prefer Terraform over Pulumi for infra."
FEW_SHOT_OUT = json.dumps(
    {
        "facts": [
            {"text": "The API was deployed to AWS on Friday.", "type": "action"},
            {"text": "The user prefers Terraform over Pulumi for infrastructure.", "type": "preference"},
        ]
    },
    ensure_ascii=False,
)

INPUTS = [
    # multi-fact tech note with identifiers
    "The build system uses make with twelve targets. Deployment happens every Friday "
    "at noon via deploy.sh. Coverage for repository/sqlite sits at 85.8 percent.",
    # decision with a reason
    "We rejected the ICU extension for SQLite because it adds a CGo dependency and "
    "only one Cyrillic test is affected; instead we documented the flakiness.",
    # preferences and opinion
    "I strongly prefer dark themes in every editor, and honestly I think tabs are "
    "better than spaces for Makefiles.",
    # Russian, mixed facts
    "Вчера мы перенесли кэш на Redis (порт 6380), бэкапы теперь идут ночью в 3 часа "
    "на NAS, а Никита решил, что миграцию схемы откладываем до пятницы.",
    # identifiers-heavy
    "TestIntegrationProductSearch is flaky: LIKE search with '%молок%' depends on "
    "SQLite Unicode behaviour. Each test uses an isolated :memory: database via setupTestDB.",
    # short single fact
    "The crate MSRV is Rust 1.85.",
]


def post_json(url, payload, headers=None, timeout=300):
    req = urllib.request.Request(
        url,
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json", "User-Agent": "helixir-bench/0.1", **(headers or {})},
    )
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.load(r)


def parse_facts(text):
    text = text.strip()
    # thinking models may prepend <think>...</think>
    if "<think>" in text and "</think>" in text:
        text = text.split("</think>", 1)[1].strip()
    if text.startswith("```"):
        text = text.strip("`")
        if text.startswith("json"):
            text = text[4:]
    try:
        data = json.loads(text)
    except json.JSONDecodeError:
        start, end = text.find("{"), text.rfind("}")
        if start == -1 or end <= start:
            return None
        try:
            data = json.loads(text[start : end + 1])
        except json.JSONDecodeError:
            return None
    facts = data.get("facts")
    if not isinstance(facts, list):
        return None
    return [f["text"].strip() for f in facts if isinstance(f, dict) and f.get("text")]


def extract_ollama(model, text):
    payload = {
        "model": model,
        "stream": False,
        "think": False,
        "format": "json",
        "options": {"temperature": 0, "num_predict": 512},
        "messages": [
            {"role": "system", "content": SYSTEM},
            {"role": "user", "content": FEW_SHOT_IN},
            {"role": "assistant", "content": FEW_SHOT_OUT},
            {"role": "user", "content": text},
        ],
    }
    t0 = time.time()
    resp = post_json(f"{OLLAMA}/api/chat", payload)
    dt = time.time() - t0
    return parse_facts(resp["message"]["content"]), dt


def extract_cerebras(text):
    payload = {
        "model": REFERENCE_MODEL,
        "temperature": 0,
        "response_format": {"type": "json_object"},
        "messages": [
            {"role": "system", "content": SYSTEM},
            {"role": "user", "content": FEW_SHOT_IN},
            {"role": "assistant", "content": FEW_SHOT_OUT},
            {"role": "user", "content": text},
        ],
    }
    headers = {"Authorization": f"Bearer {os.environ['CEREBRAS_API_KEY']}"}
    resp = post_json(CEREBRAS_URL, payload, headers)
    return parse_facts(resp["choices"][0]["message"]["content"])


def embed(texts):
    out = []
    for t in texts:
        resp = post_json(
            f"{OLLAMA}/api/embeddings", {"model": "nomic-embed-text", "prompt": t}
        )
        out.append(resp["embedding"])
    return out


def cosine(a, b):
    dot = sum(x * y for x, y in zip(a, b))
    na = sum(x * x for x in a) ** 0.5
    nb = sum(x * x for x in b) ** 0.5
    return dot / (na * nb) if na and nb else 0.0


def score(reference, candidate):
    if not candidate:
        return 0.0, 0.0
    ref_emb, cand_emb = embed(reference), embed(candidate)
    covered = sum(
        1 for re_ in ref_emb if any(cosine(re_, ce) >= MATCH_THRESHOLD for ce in cand_emb)
    )
    grounded = sum(
        1 for ce in cand_emb if any(cosine(ce, re_) >= MATCH_THRESHOLD for re_ in ref_emb)
    )
    recall = covered / len(reference) if reference else 0.0
    precision = grounded / len(candidate)
    return recall, precision


def main():
    models = sys.argv[1:] or ["qwen2.5:1.5b", "qwen2.5:3b", "phi3"]

    print("== building reference (Cerebras) ==")
    reference = []
    for text in INPUTS:
        facts = extract_cerebras(text)
        assert facts, f"reference extraction failed for: {text[:50]}"
        reference.append(facts)
        print(f"  ref [{len(facts)} facts] {text[:60]}...")

    print("\n== candidates ==")
    results = {}
    for model in models:
        # warm the model once so latency excludes cold load
        try:
            extract_ollama(model, "warmup. one fact: the sky is blue.")
        except Exception as e:
            print(f"{model}: UNAVAILABLE ({e})")
            continue
        recalls, precisions, latencies, failures = [], [], [], 0
        for text, ref in zip(INPUTS, reference):
            try:
                facts, dt = extract_ollama(model, text)
            except Exception:
                facts, dt = None, 0.0
            if facts is None:
                failures += 1
                recalls.append(0.0)
                precisions.append(0.0)
                continue
            latencies.append(dt)
            r, p = score(ref, facts)
            recalls.append(r)
            precisions.append(p)
        avg_r = sum(recalls) / len(recalls)
        avg_p = sum(precisions) / len(precisions)
        f1 = 2 * avg_r * avg_p / (avg_r + avg_p) if (avg_r + avg_p) else 0.0
        lat = sum(latencies) / len(latencies) if latencies else float("nan")
        results[model] = (avg_r, avg_p, f1, lat, failures)
        print(
            f"{model}: recall={avg_r:.2f} precision={avg_p:.2f} f1={f1:.2f} "
            f"avg_latency={lat:.1f}s json_failures={failures}/{len(INPUTS)}"
        )

    print("\n== summary (sorted by f1) ==")
    for model, (r, p, f1, lat, fails) in sorted(
        results.items(), key=lambda kv: -kv[1][2]
    ):
        print(f"  {model:18s} f1={f1:.2f} recall={r:.2f} precision={p:.2f} latency={lat:.1f}s fails={fails}")


if __name__ == "__main__":
    main()
