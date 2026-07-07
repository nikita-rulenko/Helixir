# v0.11.0 — Honest Generation

> _The generative layer earns trust: fewer false discoveries, verified
> hypotheses, and a swarm roster that tells the truth._

## The polysemy guard (#91, part 1)

Lachesis generated cross-domain "discoveries" through one word used in two
senses: *energy markets → **benchmarking** → software debugging* —
financial benchmarking fused with software benchmarking in a single
category. Measured first: embedding-based detectors **cannot** catch this
(the embedder itself conflates the senses — cohesion and bimodality both
failed on the live case), so the detector is topological. Communities are
computed over the PMI adjacency Lachesis already builds; a routed thread
truncates at a pivot whose neighbours sit in different communities with no
direct link between them. Genuine cross-domain links — endpoints adjacent
themselves — pass untouched. First live pass caught `benchmarking` twice,
plus five sibling bridges. (`lachesis.polysemy_guard`, on)

## The verification duty (#91, part 2)

Every hypothesis shipped labelled *requires verification* — and nothing
ever verified one. Now an Atropos duty reviews aging hypotheses against
their own witness memories with an adversarial judge that must cite them:

- **promote** → relabelled `VERIFIED (generated, confirmed by review)`;
- **retire** → superseded by a retirement note — and since v0.10.0 a
  superseded row auto-demotes in every search, so retired leads sink
  without being deleted;
- **keep** → the conservative default.

Witness-less hypotheses (pre-provenance debt) can never be verified —
past `atropos.verify_unverifiable_age_hours` (168) they retire as
unverifiable. Prompts teach the three labels' trust semantics.
(knobs: `atropos.verify_min_age_hours` 48, `verify_max_per_pass` 3;
daemon cadence `verify_every_passes` 6)

## Swarm presence lifecycle (#84)

One-shot agents never said goodbye — seven zeroclaw jobs sat "working"
for 15–30 hours. Three honest layers:

- **Derived staleness**: roster rows carry `derived_status` — an inactive
  agent still stored as "working" shows `stale (last reported: working)`.
  Display-level; nothing is mutated.
- **Farewell**: the new `agent_farewell(agent_id)` tool (22nd) stamps
  `done` on exit; prompts teach one-shot agents to use it.
- **Operator prune**: `helixir prune-agent --agent-id X --yes` deletes a
  junk presence row (test agents, renamed identities) via the new
  `dropPresenceByAgentId` query; refuses without `--yes` and explains
  that provenance edges die with the node.

## Charter review (#34, final piece)

`helixir charter` prints the whole learning surface: adopted rules,
precedent counts by shape, and each shape's ripeness — *rule adopted /
proposal ripe / N more verdicts to a proposal*.

## Field-tested resilience

This release was verified with a **dead Cerebras account** (402 Payment
Required): the v0.8.0 fallback chain cascaded to DeepSeek automatically —
writes complete, ~2-3× slower, zero intervention. The chain works exactly
as designed under a real outage.

## Upgrade

Drop-in from v0.10.x for binary users: replace the binary, **restart your
MCP client** (it caches tool schemas — `agent_farewell` appears after the
restart). **Self-hosted HelixDB needs a schema redeploy** for the prune
query (`helix check` → rebuild image → recreate container; the data
volume is preserved — verified live). All new config keys are optional
with safe defaults.
