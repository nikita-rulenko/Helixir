# v0.18.0 — The Maintained Substrate

Released 2026-08-25.

Helixir v0.18.0 makes the database beneath agent memory an owned, tested part
of the product. The release ships a maintained HelixDB v2.3.5 fork, fixes the
query pattern that caused database memory amplification, and adds differential
and profiling gates that distinguish Helixir growth from database growth before
a release can pass. It also completes the active memory charter, keeps default
generation on `gpt-oss-120b`, restores the shared MCP gateway after host reboot,
and prevents exact lexical memories from disappearing during graph reranking.

## A maintained database substrate

- The release repository now contains the pinned HelixDB v2.3.5 source fork,
  compiler and AGPL license. Server releases publish an immutable multi-platform
  database image, its exact source archive and a checksummed descriptor that binds
  the image digest, schema contract and fork revision.
- Existing volumes reconstruct missing secondary indexes once at startup.
  Indexed `memory_id` and `content_key` routes no longer compile to full-label
  scans that deserialize the corpus into per-request arenas.
- The maintained compiler preserves scalar versus collection response shapes
  for bulk indexed `UPDATE`; all changed rows are persisted and returned.
- Atropos uses bounded vector candidates, exact cosine, graph-backed RBAC
  domains and local NLI instead of invoking the public recall pipeline once per
  seed.
- Managed-local upgrades are backup-first and rollback-safe. Existing-local
  and remote databases remain externally owned, and `helixir-client` never
  takes ownership of HelixDB.

## Memory evidence instead of OOM guesswork

- The standalone Rust `helixdb-mock` covers the complete checked-in HQL route
  registry with bounded deterministic fixtures and compatible wire envelopes.
  Unknown routes remain errors instead of becoming false-green shims.
- `PROFILING.md` defines faithful and diagnostic lanes, symbolized builds,
  private artifact handling and an 85 percent fail-closed memory boundary.
- Release gates sample workload and database memory separately, forbid the
  production port and volume, bound Cargo cache growth, check disk headroom and
  clean disposable artifacts on every exit path.
- The faithful cold ordered workload — baseline, Clotho, insights,
  reconciliation and merge — completed without OOM, restart or functional
  query errors. The database peaked at 137.4 MiB in the final six-scenario gate
  inside its 3 GiB envelope.

## Search that keeps lexical evidence

- Native BM25, RRF, real cosine and final graph score remain independently
  observable.
- Real cosine is blended with bounded lexical evidence for direct retrieval
  seeds. The pre-PPR score and BM25-backed hybrid semantic score are floors for
  those seeds only; graph-expanded rows receive no artificial boost.
- Exact or vector-weak memories therefore survive freshness and PPR reranking,
  while explicit event-time windows and flagged graph flashbacks retain their
  existing semantics.
- The year-old `GOLDOLD` fixture guards this behavior in every search mode.

## Governed writes and predictable models

- Memory charter v1.0 is active and enforced in Rust plus atomic HQL mutation
  guards. Immutable memories, system seeds, learned rules and preserved
  `raw_input` sources cannot be silently rewritten or superseded.
- Interrupted seed runs resume and promote compatible legacy rows
  idempotently; blocked mutations do not emit false history or events.
- Cerebras `gpt-oss-120b` remains the default reasoning/write model.
  Generation fallback is disabled and empty unless explicitly configured.
  Ollama/Nomic embeddings and the mandatory local NLI judge retain their
  server-side roles.

## One gateway after reboot

- `helixir gateway start` installs a launchd service on macOS or a systemd user
  service on Linux. Repeated start/install operations replace the definition
  without creating duplicate listeners.
- Status checks validate the managed service PID and exact executable identity
  instead of trusting a stale PID file or an unrelated process on the port.
- The release gate boots the packaged Linux archive inside a privileged
  Ubuntu/systemd container, restarts the complete user manager, repeats MCP
  discovery, and rejects a reachable port without the canonical
  `helixir-gateway.service` owner.
- A full macOS reboot and forced launchd restart restored exactly one gateway;
  MCP initialize, all 23 tools, heartbeat and memory recall passed afterward.

## Release and E2E hardening

- The canonical permanent-RBAC matrix owns 60 declared scenarios and refuses
  manifest drift. Golden graph fixtures, Hive consensus and MCP write assertions
  no longer depend on private dogfood state or nondeterministic LLM wording.
- The final clean current-schema run passed the full applicable matrix,
  including 23/23 MCP tools, concurrent reads, Hive consensus, RBAC
  isolation/federation/history, NLI, Moirai/Lachesis, schema inventory, swarm
  lifecycle and read/write quality gates.
- The deterministic surface passed 449 Helixir tests, 11 client tests, 18
  maintained-fork tests, 34 mock tests, 60 memory-boundary Python tests, 15 web
  unit tests and 24 browser scenarios, plus strict formatting, Clippy, rustdoc,
  package and safety preflights.
- `helixir config set mode` now canonicalizes documented lower-case CLI values
  into the Serde-safe TOML enum representation, and the remote-client gate binds
  the exact maintained HelixDB CLI instead of depending on ambient `PATH` state.

## Upgrade

This release keeps the physical 22-node, five-vector and 30-edge model, but the
compiled HQL surface is now **192 queries** and the managed database engine has
changed. A full host must upgrade the database, server binaries, gateway and
control plane as one backup-first transaction:

1. stop writers and create a verified cold backup of the persistent volume;
2. rehearse that backup with the exact v0.18.0 HelixDB image on an isolated
   daemon and alternate port;
3. verify memory counts, permanent RBAC, schema marker/census, indexed lookup,
   charter guards and a clean restart;
4. replace the managed database against the original volume, then replace the
   Helixir binaries and control plane;
5. start exactly one managed gateway and watchdog, run `helixir doctor --json`,
   and verify read/write plus protected-memory behavior;
6. restart Codex, Claude Code and Cursor so they reload the current MCP surface,
   prompts and installed skill.

Agent-only hosts upgrade `helixir-client` normally. They receive the client
binary, MCP registration, `AGENTS.md` integration and the canonical skill, but
do not install HelixDB, NLI, embeddings, Moirai, Hygieia or the control plane.
