# v0.13.2 — The Guarded Reload

This patch closes runtime audit findings and completes the first reusable
installer/onboarding slice. Reload generations could overlap their ingest
workers (#98), while the network gateway had no optional guard for operators
who do not fully trust their network boundary (#99).

## One runtime generation, one ingest worker

SIGHUP now publishes the client, FastThink manager, and tooling manager as one
coherent runtime generation. The process owns exactly one ingest worker, and
that worker follows the currently published client instead of multiplying on
every reload. Existing FastThink sessions remain pinned to the generation that
created them; newly started sessions use the new configuration.

The queue claim is now atomic in HelixDB through `claimPendingInput`, so two
processes cannot successfully claim the same pending row. Stale `processing`
rows are still recoverable after the configured timeout.

## Optional bearer guard, trust by default

Gateway bearer authentication is available but remains disabled by default, in
line with Helixir's trusted-network deployment model. Operators can enable it
with `gateway.auth_token`, `HELIXIR_GATEWAY_TOKEN`, or `helixir config`; the
token hot-reloads with the rest of the runtime and is redacted from config
output. `helixir gateway --require-auth` makes a missing token a startup error
for deployments that must fail closed.

## Guided embeddings and bounded CLI modules

Onboarding defaults to Ollama plus `nomic-embed-text`, but also accepts an
explicit OpenAI-compatible remote embedding provider, model, URL, and protected
API key. NLI and a working embedding path remain mandatory. Doctor performs a
real embedding probe; a broken remote path is reported and recovered by
installing/starting Ollama, pulling Nomic, atomically switching the central
config, and verifying the result.

Ollama installation and startup now use platform adapters, interrupted model
pulls retry through the official API, and final inventory is verified through
`/api/tags`. The CLI god-module was split into domain modules under
`src/bin/helixir/`; a test enforces a 500-line maximum per CLI module.

## Verification

- 250 library tests and 7 CLI tests pass.
- The complete non-ignored all-target test surface passes.
- Formatting, documentation lint, the exact CI Clippy gate, and default plus
  no-default all-target builds pass.
- HelixDB CLI v2.3.5 compiles all 137 HQL queries.
- Live smoke verified exclusive atomic claims and gateway responses: `401` for
  missing or incorrect bearer credentials, `200` for a valid MCP initialize.

## Upgrading

Self-hosted installations must redeploy the schema because this patch adds
`claimPendingInput`. Stop the instance, back up its persistent data volume, run
`helix check`, rebuild/recreate the HelixDB container against the same volume,
then replace the Helixir binary and restart MCP clients and gateways.

No configuration change is mandatory. Authentication remains off unless a
token is configured; use `--require-auth` only where fail-closed startup is
desired.
