# v0.8.0 — The Resilience Release

> _Supersedes `v0.7.0`. The memory now survives everything short of the
> laptop catching fire — and remembers why it had to._

Four feature waves shipped on dev since Hygieia; together they answer one
question: **what does it take for an agent's memory to keep working when
providers die, quotas run out, and the only model left is a 2GB local one?**

## Composable fallback chain — smart remote → cheap remote → local selfhost

On **any** primary-LLM error — network outage or exhausted quota — the same
prompt cascades down an ordered provider chain (default
`cerebras → deepseek → ollama`) and readopts the primary on its first
successful call. The system degrades under outage and heals on its own.

- Tiers missing credentials are **skipped at boot with a warning**, never a
  boot failure: without a DeepSeek key the chain simply degrades to local
  Ollama.
- A fallback answer's metadata carries the **full error trail**
  (`cerebras: 401; deepseek: timeout`) — you always know why a weaker model
  answered.
- Live-verified both ways: dead Cerebras → DeepSeek finished the full add
  pipeline in ~20s; dead Cerebras **and** DeepSeek → local Ollama still
  landed the write.

```toml
llm_fallback_chain = ["deepseek", "ollama"]   # or HELIX_LLM_FALLBACK_CHAIN
deepseek_api_key   = "sk-..."                 # or HELIX_DEEPSEEK_API_KEY
```

## llama3.2:3b — the new local floor (M1 bake-off verdict)

Seven small models went through the full add pipeline, the causal-contract
e2e and a Russian-language probe on the target laptop class (M1, 16GB).
**llama3.2:3b** takes the local-fallback default from qwen2.5:7b: causal
contract green at ~2× the speed and half the RAM, walkable causal chains,
no fabrication. Runner-up qwen3.5:2b writes the richest atoms but is 4×
slower under contract load. The bake-off also caught a real failure mode —
the weakest models can copy the extraction prompt's worked example into a
stored memory as a fabricated fact (#79) — and the Ollama path now always
sends `think: false`, so thinking-family models answer instead of
monologuing.

## Time governs attention, never reachability (#31, #76)

The temporal redesign lands the elder-brain principle in code. No preset
hard-cuts history anymore: search modes carry a **temporal weight**
(freshness biases ranking), while explicit `temporal_days` remains a hard
window on seeds only. Memory is **bi-temporal**: freshness and windows
follow *event time* (`valid_from`, else `created_at`), so a fact ingested
yesterday about last year ranks like last year's fact. Guarded by a
deterministic, LLM-free **golden corpus net** (24 fixtures, marker-based
assertions) that caught a real full-mode bug on its first run.

## Weak-model write path, hardened (#78, #66)

The flaky causal-extraction contract went from 1-in-2 to deterministic:

- **Connective backstop** — if an explicitly causal message stores ≥2 atoms
  and the whole pipeline produced zero relations, a `BECAUSE` edge is wired
  deterministically by clause alignment (EN + RU connectives).
- **Tolerant extraction serde** — missing `entities`, entity objects instead
  of id strings, float certainty, context-as-object: coerced, not rejected.
  Unit-tested on the exact payloads observed from live weak-model traffic.
- **Worked example** in the extraction prompt (there was a schema but no
  sample to copy) + a causal MUST rule.
- **FastThink overflow trap removed** — `think_conclude` bypasses the
  thought cap: the conclusion is the *exit*, and the limit error now guides
  to conclude-or-discard instead of dead-ending the session.
- The **swarm entered the prompts**: cognitive protocol and integration
  templates now teach `agent_id`, `swarm_status`, `list_users` and
  `pending_outcomes` — the rendezvous existed, now agents are told to use it.

## Charter increment 2a — defer, don't destroy (#34)

With `write.charter_blocking = true` (the new default), destructive verdicts
on charter-protected memories are not applied — the new fact is **added
alongside** and linked by a `charter_deferred` CONTRADICTS edge, surfaced as
a clarification question. The new `resolve_contradiction` tool settles the
debt three ways: `confirm` (owner blesses the change), `retract` (supersede
the newcomer), `preference` (both coexist). The charter itself is now
servable as the `memory://rules` MCP resource (operator-overridable at
`~/.helixir/memory-charter.md`) and its core articles ride inside the write
decision prompt.

## Also

- `dropMemoryCascadeByInternalId` — the operator repair path for debug
  artifacts (used to retire the OOM-era flood insights, #71).
- CI clippy job unstuck (deny-by-default lint on a dead kill-switch branch).
- 152 lib tests (+10 chain/resolver/env units); 31 e2e suites green in the
  pre-wave full regression.

## Upgrade

Drop-in. New config keys are optional with safe defaults; if you relied on
the implicit qwen2.5:7b local fallback, either `ollama pull llama3.2:3b` or
pin `llm_fallback_model = "qwen2.5:7b"`.
