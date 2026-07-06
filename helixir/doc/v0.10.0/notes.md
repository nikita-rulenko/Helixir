# v0.10.0 — The Learning Charter

> _The constitution stays human. The rules grow from your verdicts._

A minor release with a loop inside: the charter that guards your memory
now **learns from how you settle its questions** — and the store finally
lets corrections beat the facts they corrected.

## The charter learns (#34 increment 2b)

- Every `resolve_contradiction` verdict is recorded as a **precedent**: an
  episode memory tagged with the dispute's shape (new-type / old-type /
  verdict), SUPPORTS-linked to both disputed memories — "why does this
  rule exist" walks to the evidence.
- After `write.rule_propose_after` identical verdicts (default 3), the
  resolve result carries a **`rule_proposal`**: a standing rule ready to
  adopt with the exact `add_memory` call it dictates.
- Adoption is **verbatim**: a message starting `Charter rule [shape]:`
  takes a deterministic single-atom path (no LLM rephrasing) and renders
  in the `memory://rules` resource **beside** the constitution — which
  itself never self-learns. An adopted rule silences further proposals of
  its shape.

## Corrections win (#92)

An append-only store made stale hubs immortal: a densely-linked old fact
carried PPR mass its own corrections could not beat. Now a row with an
incoming SUPERSEDES edge ranks below its successor
(`retrieval.superseded_penalty`, 0.6) and returns honestly flagged:
`superseded: true` + `superseded_by: <id>`. Reachability untouched —
history is labelled, never hidden. **Self-hosted deployments must
redeploy the schema** (new `getSupersededBatch` query).

## The charter stops crying wolf (#93)

The decision engine over-eagerly flagged same-subject elaborations as
conflicts (three false clarifications in one working day, pinned as
regression fixtures). A conflict now requires a shared subject AND a
near-restatement (raw-cosine floor 0.88); everything else quietly ADDs.
Genuine reversals still escalate — now with `new_memory_id` in the
notice, so `resolve_contradiction` is deterministic.

## The write path got cheap (#96, Levers 1–2)

- **Batched relation inference**: one LLM call for all new atoms
  (was one per atom).
- **Reliable batch decisions**: dense prompt indices + one batched repair
  retry — the per-item fallback cascade is gone (measured: the same write
  went from 9 calls/$0.0070 to 4 calls/$0.0038).
- **NLI edge routing**: the local deberta judge types confident
  SUPPORTS/CONTRADICTS pairs before the LLM is consulted — bidirectional
  entailment, subject-gated contradictions (`write.nli_route`, on;
  self-gating no-op on lean builds). Proven by a new e2e that builds a
  typed edge **with a dead LLM key** — the amplifier thesis in one test.
- A per-call token instrument (`helixir::llm::cost`) makes future cost
  work measurable.

## Every ontology type earns its keep (#94, #95)

All 8 memory types classify correctly — measured 16/16 on EN+RU labeled
sentences on **both** cerebras gpt-oss-120b and llama3.2:3b, and a mixed
milestone paragraph keeps all 6 non-fact types distinct on both models.
Prompts now teach *writing for the ontology* ("I prefer / I can / my goal
/ I think / I realized / I shipped") — typed memories are findable
memories. Relation inference no longer loses edges under `json_object`
mode (#95).

## Quality net

New gated e2e: charter learning loop (two-phase: fresh store grows a
rule / mature store proves silencing), supersede demotion (LLM-free),
NLI routing with a dead LLM, ontology classification (16 sentences + 
mixed paragraph). The NLI e2e caught a real production bug on its first
run (`block_in_place` panics on current-thread runtimes) — fixed via
`spawn_blocking`.

## Upgrade

Drop-in from v0.9.x for binary users: replace the binary, restart your
MCP client. **Self-hosted HelixDB needs a schema redeploy** for the
superseded-demotion query: `helix check` → rebuild the image → recreate
the container (the data volume is preserved; verified live on a 1204-
memory store). All new config keys are optional with safe defaults.
