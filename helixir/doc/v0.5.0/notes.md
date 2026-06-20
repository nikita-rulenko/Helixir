# v0.5.0 — the collective release

> _Supersedes `v0.4.0`. The theme: many minds, one memory — shared
> consensus without races, a network surface, and provider freedom._

v0.4.0 made memory that reasons locally. v0.5.0 makes it **collective**:
the same fact held by many agents converges to one consensus group, the
store is reachable over the network, every hardcoded knob now lives in
config, and the LLM is no longer tied to a single vendor.

## Collective memory & dedup (#43, #44)

- **Content-keyed fingerprinting**: each memory carries a deterministic
  `content_key = sha256(normalize(content) + memory_type)`. Identical
  facts across users share the key; each user keeps their **own** node, so
  personal vector search is unchanged and there is **no shared-node
  snapshot race**.
- **Collective `user_count` derived on read**: consensus = the number of
  distinct holders in a `content_key` group, computed at query time — not
  a mutable counter that two writers can clobber.
- **Atomic dedup**: content-keyed schema + queries (validated against
  HelixDB), with a `backfill` that stamps `content_key` onto pre-existing
  rows (null-safe for legacy records).
- **Dedup is now visible**: a NOOP write surfaces the existing memory it
  deduped to in `add_memory.deduped[]`, so the agent sees "linked to X"
  instead of a silent empty result.

## NLI paraphrase backstop (#55)

- **Local NLI judge, no Python**: a DeBERTa cross-encoder runs in pure
  Rust via `ort` (ONNX). It labels entailment / contradiction / neutral
  and only ever merges on mutual entailment — never across a contradiction.
- **The repo ships no weights**: a platform-aware downloader (CLI:
  `helixir model download|status|check|which`) fetches the right quantized
  ONNX for the host arch on demand, into `~/.helixir/models/`.
- **Always-on in the daemon**: Atropos runs a content-key paraphrase merge
  every reconcile pass, gated on the collective/insights tier and budgeted
  via config (`moira.daemon.merge_limit`, `merge_cosine_threshold`).

## Network gateway (#42)

- **MCP over the network**: the same handler is served over
  streamable-HTTP, not just stdio. Background lifecycle
  (`gateway start|stop|status`) and `setup --gateway` wire clients to a
  gateway URL over HTTP.

## Privilege tiers

- **`HELIXIR_MODE` = solo | collective | insights** (default solo).
  Solo never reaches across users; collective enables cross-user linking
  and consensus; insights adds the aggregate layer. Surfaced via
  `helixir mode` and `setup --mode`.

## Disputes (#45)

- **Superseded factual disputes drain to the owner's outbox**: when a
  stored fact is contradicted, the temporal signal is surfaced to the
  owner instead of resolved silently.

## LLM providers

- **External→local fallback (wired)**: if the remote primary (Cerebras /
  DeepSeek) errors, the same prompt transparently retries against local
  Ollama. The wrapper existed but was never constructed — a remote outage
  used to fail the write. Skipped when the primary is already Ollama.
- **DeepSeek as a provider**: Cerebras and DeepSeek share one
  `OpenAiCompatProvider` (same OpenAI wire format, parameterized base URL).
  DeepSeek defaults to `deepseek-v4-flash` in **non-thinking** mode for
  clean, fast JSON — ~$0.0001/extraction, cheaper than Cerebras and far
  faster than local. Config-driven: `HELIX_LLM_PROVIDER=deepseek`.
- **Validated local fallback model** = `qwen2.5:7b` (was the dead
  `llama3.2`). Full e2e on local: 7b passes the core write/read suite and
  closes the extraction-recall + categorisation gaps that 3b drops; 3b
  sits below the ~7-8B reliability cliff. Deep multi-step reasoning
  (`think_commit`) is a remote-only capability — it degrades on every
  local model regardless of size.
- **Default model** = `gpt-oss-120b` (the prior `llama-3.3-70b` was
  retired from Cerebras → fresh installs couldn't extract).

## Config consolidation (0-hardcode)

- **One config surface, layered loader** (defaults → TOML → env): every
  live tunable that used to be a scattered `const` now lives in nested
  config groups — retrieval search-mode presets, PPR alpha/iterations,
  edge weights & damping, the write/add-pipeline literals, FastThink
  session limits, chunking, LLM runtime knobs, DB retry, charter
  thresholds, entity/reasoning caps, the Moirai.
- **Dead-config goal met**: const modules that duplicated config
  (`edge_weights`, the per-Moira consts) were deleted, not shadowed —
  verified by "0 dead config fields".

## Known issues (pre-existing, tracked)

- `mcp_write_e2e:102` (charter C3 preference-reversal escalation) is
  LLM-decision-dependent (#52): the escalation only fires when the model
  classifies the reversal as an Update/Supersede of a protected type; for
  some inputs the model chooses ADD and no escalation is raised. The
  charter logic itself is deterministic and unit-tested.
- `lachesis_gate_e2e:67` depends on the live store containing a
  reasoning-backed pair; it can read `reasoning_support = 0` on a store
  whose seed chain has decayed. Both fail identically on v0.4.0 — neither
  is introduced here.

## Testing posture (carried forward)

The 22 e2e suites are gated behind `HELIX_E2E=1` and are **not** run by CI
(`cargo test --lib` only). A test-strategy pass — CI e2e gate on a seeded
DB + cheap provider, negative-path harness support, and pulling the
deterministic core (content_key, charter, decision parsing, providers)
down into unit tests — is the next planned work.
