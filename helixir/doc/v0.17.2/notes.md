# v0.17.2 — The Enforced Charter

Released 2026-08-23.

Helixir v0.17.2 turns the approved memory charter into an end-to-end mutation
contract and hardens the release path around the system that owns the live
graph. Protected knowledge now remains protected even if a caller bypasses the
normal Rust preflight, generation stays on the operator-selected `gpt-oss`
path by default, and administrators can finish client placement from the
control plane without reconstructing CLI state by hand.

## Enforced charter v1.0

- The charter is published as active v1.0 instead of a historical draft.
- Immutable memories and preserved `raw_input` sources cannot be updated,
  superseded or deleted through direct APIs, the decision pipeline, or legacy
  HQL mutations.
- System seeds and adopted charter rules are created immutable atomically.
  Interrupted seed runs resume, fill missing rows and promote compatible
  legacy rows instead of treating one partial marker as completion.
- Destructive verdicts normalize one canonical target before policy checks and
  persistence. A blocked mutation no longer emits false supersession history.
- Dedicated deterministic and disposable live contracts cover C2/C4 behavior.

## Predictable model routing

- The default reasoning/write path uses Cerebras `gpt-oss-120b` only.
- Generation fallback is disabled with an empty chain unless the operator
  explicitly configures one.
- Ollama, Nomic embeddings and local NLI keep their server-side roles; default
  onboarding no longer pulls an unrelated local generation model.

## Faster client administration

- The admin-only control plane shows the advertised MCP gateway host and port,
  a copyable connection command and a fresh-onboarding registry.
- A global administrator can place a newly enrolled principal into an existing
  or explicitly created working group with its graph-backed role from the same
  access surface.
- Secrets remain server-side: the UI exposes connection coordinates, never the
  optional bearer token.

## Release safety

- The Docker-heavy client gate refuses every daemon that already owns
  containers or volumes unless the whole daemon is explicitly declared
  disposable.
- The deterministic preflight is part of release CI and distinguishes an
  engine disappearance inside a Docker command from an ordinary test failure.
- HelixDB is built once per gate. The former redundant second Docker build was
  removed.
- A branch validation run exports exact ARM64 HelixDB and control-plane
  candidate images. Operators can rehearse a restored backup in isolation
  before replacing a dogfood runtime, without compiling on the production
  Docker daemon.
- If artifact transport is unavailable, the exact-source fallback now refuses
  a non-empty daemon or fewer than 4 GiB effective RAM plus explicit memory and
  swap assertions, constrains the generated HelixDB and control-plane builds
  to one Cargo job, compiles the control plane from the exact commit archive
  and exports checksummed candidates.

## Upgrade

The physical node/vector/edge declarations remain unchanged. The compiled HQL
surface grows from 185 to **189 queries** to enforce protected mutations and
resumable immutable seed state. Full hosts must create and verify a cold backup,
rebuild the HelixDB v2.3.5 image from this exact release, rehearse the restored
volume, then swap the database, binaries, gateway and control plane together.
Run `helixir doctor --json`, verify the schema census and perform a protected
charter write/read check before resuming normal writers.

Agent-only hosts update `helixir-client` normally; they do not deploy HelixDB,
NLI, embeddings, Moirai, Hygieia or the browser control plane.
