# Upgrading Helixir

> ⚠️ **HelixDB version pin: CLI v2.3.5.** Helixir targets the v2 (LMDB)
> generation of HelixDB; **v3.x is incompatible** (different engine, no
> `helix check`/`helix build`, schema never registers — `query_count: 0`).
> Every `helix …` command in this file assumes CLI 2.3.5 — see the
> [installation prerequisites](helixir/doc/installation.md#prerequisites) for
> the pinned install command. Do NOT `helix update`.

> ⚠️ **Before ANY upgrade that touches HelixDB itself:** newer HelixDB builds
> default to **in-memory storage** — stopping the instance ERASES everything
> unless it runs with disk persistence (`helix start dev --disk`, or a mounted
> `HELIX_DATA_DIR` for containers as our compose/install configure). After the
> upgrade, verify: write a memory, restart the instance, confirm it survived.

## v0.16.0 → v0.16.1 — one host, one gateway

v0.16.1 is a binary and MCP-client-registration patch. It does not change the
HelixDB schema or memory/RBAC data. Upgrade the binaries, start the host-local
gateway, migrate supported clients, run doctor, and restart them:

```bash
helixir gateway start --bind 127.0.0.1:8765
helixir setup --gateway 127.0.0.1:8765
helixir doctor --json
```

The setup command backs up and verifies conflicting Codex and Claude Code
registrations. Stdio remains available for clients without streamable HTTP.

## v0.15.0 → v0.16.0 — distribution and stewardship

v0.16.0 is a binary/control-plane upgrade. It does not change the HelixDB
schema contract or rewrite memory/RBAC data. It adds signed Homebrew and APT
distribution, a persistent versioned embedding cache, one installer service
shared by CLI and browser flows, a hardened versioned admin API, redacted
post-install settings, and a managed backup vault.

The v0.16.0 release workflow publishes and validates both signed package
channels after the native release artifacts are immutable. If a downstream
mirror is temporarily unavailable, use the signed release installer rather
than mixing files from multiple versions.

Choose one package path:

```bash
# macOS or Linuxbrew
brew update
brew upgrade helixir

# Debian 12 / Ubuntu 22.04+ after adding the repository from README.md
sudo apt update
sudo apt install --only-upgrade helixir
```

The package manager owns immutable binaries and packaged runtime resources
only. It does not remove or replace `~/.helixir`, HelixDB volumes, models,
backups, central configuration, or MCP registrations. Run the shared
orchestrator after the package transaction:

```bash
helixir onboard
helixir doctor --json
helixir control-plane status
```

When RBAC is already active, repeat onboarding reuses the global administrator
recorded in `~/.helixir/install.json`; pass `--rbac-operator` or
`HELIXIR_RBAC_ACTOR` only to select another existing global administrator.

Existing-local and remote HelixDB installations remain observe-only in the
web backup vault. Only a Helixir-managed local database can create, verify, or
restore managed archives. Restore always requires the exact phrase
`RESTORE <archive-id>`, creates a safety snapshot first, and automatically
restores that snapshot if the recovered database fails the live schema probe.

The persistent embedding cache is opt-in through
`HELIXIR_EMBED_CACHE_PATH`. Its namespace includes the format version,
provider, normalized endpoint, model, artifact revision, vector dimension and
`HELIXIR_EMBED_CACHE_EPOCH`; a changed identity misses safely rather than
reusing an incompatible vector. Increment the epoch when an opaque remote
provider changes model weights without exposing a revision.

Restart every MCP client after replacing the binaries. MCP clients cache the
server process, tool schemas, prompts and installed skill at session start.
The control-plane supervisor reloads supported settings automatically and
reports which deeper services still need a restart.

## v0.14.3 → v0.15.0 — the memory observatory

v0.15.0 adds the containerized global-admin control plane and its native typed
supervisor. Use the normal backup-first installer flow, then run
`helixir doctor --json`. The installer publishes no HTML/JavaScript into the
native tree: it pulls the immutable GHCR image, installs the supervisor as a
launchd or systemd user service, and starts the loopback-only container. Use
`install.sh --no-web` when the host is intentionally headless.

RBAC remains permanent and HelixDB remains the only authorization source. The
upgrade preserves memories, visibility edges, role history, dedup federations,
Moirai provenance and the existing backend ownership contract. Restart MCP
clients after replacing the binaries so their cached prompts/tool schemas pick
up the v0.15 guidance. Check `helixir control-plane status` and open
`http://127.0.0.1:6971` with the private browser token printed by the installer.

## v0.14.0 → v0.14.1 — the compatible judge

v0.14.1 is a binary-only compatibility patch. It keeps the v0.14.0 schema,
configuration, RBAC graph, and persistent volume unchanged. The Rust ONNX
binding now explicitly targets API 23, matching the official ONNX Runtime
1.23.2 universal macOS package shipped by CI and release archives. This fixes
NLI startup on both Apple Silicon and Intel macOS; Linux and Windows behavior
is unchanged.

Replace all three Helixir binaries and keep the packaged ONNX Runtime library
beside them. Run `helixir doctor --json`, confirm `ready: true`, then restart
every long-lived MCP client. No HelixDB backup or schema deployment is required
when upgrading from v0.14.0; installations upgrading from v0.13.x must still
complete the v0.14.0 transition below first.

## v0.13.2 → v0.14.0 — the governed hive

v0.14.0 is a one-way transition to permanent graph-backed RBAC. There is no
disabled profile or rollback switch. HelixDB is the single source of truth for
principals, roles, groups, memory visibility, dedup federations, audit history,
and migration checkpoints.

Bootstrap creates two reserved workspaces. `default` receives pre-RBAC memories
and trusted peers as equal `groupadmin` members, preserving the old shared data
plane without granting everyone control-plane admin. `onboarding` admits new
principals before an administrator assigns working groups. Only the explicit
operator receives global `admin`. The migration records
`pending → migrating → active`, is idempotent and resumable, and never returns
to disabled enforcement.

Every MCP client must send its stable `actor_id`; `user_id` remains the memory
owner. Working-group writes name a concrete `group_id`. Omission is accepted
only when exactly one reserved workspace is writable; ambiguity fails closed.
FastThink sessions, pending writes, admin handles, roster inspection, and Moirai
surfaces enforce the same actor boundary. A timed-out FastThink session is not
auto-persisted because the timeout path has no explicit owner/group context.

Administrators use `helixir rbac user`, `helixir rbac group`, and
`helixir rbac dedup`. A new principal first joins `onboarding`; removal
deactivates grants while retaining the User node and role history. Federating
groups shares deduplication and visibility for current members. Detaching is
prospective: old group edges remain readable, while new writes return to the
group-private scope. Historical federation memories fork through supersession
instead of being mutated across a changed visibility set.

### Safe schema and runtime transition

This release requires schema contract `helixir-rbac-default-onboarding-v3`
(170 HQL queries). Before replacing binaries:

1. Stop writers and HelixDB.
2. Create and verify a recoverable cold backup of the persistent volume.
3. Confirm `helix --version` is exactly `2.3.5`, then run `helix check`.
4. Rebuild/recreate the v2.3.5 container against the same volume.
5. Verify `getHelixirSchemaVersion`, RBAC `enabled + active`, and read-only
   memory/user counts before restarting writers.
6. Install the new binaries, run `helixir doctor --json`, and restart every
   long-lived MCP client so it reloads tool schemas and prompts.

The installer distinguishes a Helixir-managed local database, an existing
separately managed local database, and a remote database. Managed local
transitions are backup-first and transactional. The product default remains
port 6969; explicitly detected endpoints such as 6970 are preserved.

### Required models and bounded HelixDB v2 memory

NLI is mandatory in every build and installation. Embeddings must be either
verified Ollama with `nomic-embed-text` or an explicit working
OpenAI-compatible remote endpoint. If remote embedding recovery fails,
`helixir doctor` visibly installs/starts Ollama, pulls Nomic, switches the
central config atomically, and verifies the repair. Cerebras generation is
pinned to `gpt-oss-120b`; Gemma is never selected by Helixir.

Helixir v0.14.0 removes repeated label scans from hot graph/RBAC paths and
caches an atomic policy snapshot by the graph-backed policy revision. HelixDB
v2.3.5 `SearchV` still retains a smaller request high-water upstream, so managed
containers use one visible core, eager mimalloc decommit, a 3 GiB hard cap, and
Hygieia's volume-preserving pre-OOM restart. `helixir watch install` carries
the manifest operator identity and deterministic Docker path into launchd or
systemd.

## v0.4.x → v0.13.2 — schema note for v0.13.2

Every release from v0.5.0 through v0.13.1 upgrades in place. v0.13.2 adds
one HQL query, so self-hosted deployments must back up and redeploy the
schema before replacing the binary. New config keys remain optional with
safe defaults. Version-by-version notes, newest first:

| Version | Theme | Worth knowing when upgrading |
|:--------|:------|:------------------------------|
| **v0.16.1** | One host, one gateway | HTTP-capable MCP clients share one managed gateway instead of accumulating children behind retained stdio pipes. No schema migration; start the gateway, run `setup --gateway`, doctor, then restart clients. |
| **v0.16.0** | Distribution and stewardship | Signed Homebrew/APT channels, shared CLI/browser onboarding, versioned persistent embedding-cache invalidation, hardened admin API, redacted settings, and guarded managed-volume backup/restore. No schema migration; upgrade binaries, run onboard/doctor, then restart MCP clients. |
| **v0.15.0** | The memory observatory | Global-admin-only web control plane and typed native supervisor. The container has no Docker socket or host-home mount; graph RBAC stays the authorization source. Run doctor, verify `helixir control-plane status`, then restart MCP clients. |
| **v0.14.1** | The compatible judge | Binary-only patch: NLI now targets ONNX Runtime API 23, matching the universal macOS runtime in release archives. No schema or data migration; replace binaries, run doctor, and restart MCP clients. |
| **v0.14.0** | The governed hive | Permanent graph-backed RBAC introduces reserved `default` and `onboarding`, group roles, dedup federations, actor-bound MCP/FastThink, administrative CLI, transactional onboarding, mandatory NLI plus verified embeddings, and bounded HelixDB v2.3.5 memory. **Cold-backup and deploy schema v3 before replacing binaries; then run doctor and restart every MCP client.** |
| **v0.13.2** | The guarded reload | Hot reload now publishes one coherent runtime generation while one process-owned ingest worker follows the active client; an atomic `claimPendingInput` query prevents duplicate queue work across processes. **Back up the data volume and redeploy the schema** before replacing the binary, then restart MCP clients/gateways. Gateway bearer auth is optional and off by default; enable it with `gateway.auth_token`, `HELIXIR_GATEWAY_TOKEN`, or `helixir config`, and use `helixir gateway --require-auth` when startup must fail closed. |
| **v0.13.1** | The honest valve | The Hygieia cache valve and `memprobe --reclaim` now ask cgroup reclaim for the FULL current charge instead of a fixed 1024MiB step — under-asking produced false "live heap" verdicts and premature restarts (#89 forensics). Restart a running `helixir watch` to pick it up. |
| **v0.13.0** | The self-steering release | `helixir config get/set/edit/apply` hot-reloads running MCP/gateway processes via SIGHUP (client rebuilt from the re-read `helixir.toml`, swapped atomically) — **restart MCP clients once on this binary before your first `apply`** (older binaries exit on SIGHUP). Hygieia self-restarts the database container on genuine live-heap pressure (`watchdog.mem_restart_pct`, 92; needs `allow_container_restart`). linux-x86_64 + windows artifacts are full-featured again (NLI; the ONNX runtime ships in the tarball — keep it next to the binaries). `chunking.enable_embeddings` removed (the machinery was dead, #86). |
| **v0.12.0** | The operator release | Ops alerts can push to a human: `watchdog.on_alert_cmd` runs on every alert with `HELIXIR_ALERT_KIND`/`HELIXIR_ALERT_SUMMARY` in the env (off when empty). `helixir watch install`/`uninstall` runs the watchdog as a launchd agent / systemd user unit (refuses `target/` binaries). FastThink recall reserves `fast_think.conclude_reserve` (2) thoughts of headroom so synthesis always fits. Default logs are ASCII; `helixir-deploy` is a clap CLI (`-h` = `--help`, `--version`, invalid `--port` errors). |
| **v0.11.0** | Honest generation | Lachesis truncates threads at polysemous pivot categories (`lachesis.polysemy_guard`, on). Atropos verifies aging hypotheses — promote to `VERIFIED` / retire via SUPERSEDE (`atropos.verify_*` knobs, daemon `verify_every_passes`, 6). New `agent_farewell` tool (22nd) — restart your MCP client for the schema; roster rows gain `derived_status`. Operator prune: `helixir prune-agent` — **self-hosted deployments must redeploy the schema** (new `dropPresenceByAgentId`). `helixir charter` reviews learned rules. |
| **v0.10.0** | The learning charter | The charter grows rules from your `resolve_contradiction` verdicts (`write.rule_propose_after`, 3; adopted rules render in `memory://rules`). Superseded facts rank below their corrections, flagged `superseded`/`superseded_by` (`retrieval.superseded_penalty`, 0.6) — **self-hosted deployments must redeploy the schema** (new `getSupersededBatch` query: `helix check` → rebuild image → recreate container, volume preserved). Charter false positives are gated (shared subject + 0.88 similarity floor). Write-path LLM cost drops: batched inference + reliable batch decisions + local-NLI edge routing (`write.nli_route`, on; no-op on lean builds). All 8 ontology types classify correctly even on llama3.2:3b. |
| **v0.9.2** | Flashbacks | `search_memory` gains `time_from`/`time_to` event-time windows; out-of-window rows reachable via edges return flagged `flashback` (cap `retrieval.flashback_max`, 3). Restart your MCP client — it caches tool schemas. Rerank on dense graphs is capped (`retrieval.rerank_max_rows`, 128). `think_recall` gains an annotated weak-evidence fallback (`fast_think.recall_fallback_*`). Hygieia cache valve is opt-in (`watchdog.allow_cache_reclaim` — spawns a privileged helper). Old compose files reference a Docker Hub image that never existed — re-run `install.sh` or take the new compose. |
| **v0.9.1** | The honest arsenal | 12 dead edge types cut from the schema; self-hosted deployments should redeploy the schema (`helix check` → push, volume preserved). Explicit "is part of"/"is a kind of" (EN+RU) now guarantee PART_OF/IS_A edges; the example-leak firewall drops prompt-example fabrications; extraction keeps the input language. |
| **v0.9.0** | Curation | Read output is now capped/deduped/folded (`metadata.collapsed`). Raw sources written before v0.9.0 carry no family edges, so collapse benefits new writes. Lachesis gains retroactive causal stitching (`moira.daemon.stitch_every_passes`, default every 4th pass). Swarm roster hides agents silent past `swarm.presence_ttl_secs` (30 min). |
| **v0.8.0** | Resilience | LLM fallback is now an ordered chain (`llm_fallback_chain = ["deepseek", "ollama"]`, `HELIX_DEEPSEEK_API_KEY`). The local floor changed **qwen2.5:7b → llama3.2:3b** — `ollama pull llama3.2:3b`, or pin `llm_fallback_model = "qwen2.5:7b"`. Release artifacts are lean (no NLI); build from source for the NLI judge. |
| **v0.7.0** | Hygieia | Built-in health watchdog (`[watchdog]` config, `helixir watch`/`health` CLI) with autobackup. Off-by-default actions (container restart) are opt-in. |
| **v0.6.x** | The hive | Insights persist as first-class memories; swarm rendezvous (`swarm_status`, `list_users`, auto-heartbeat via `agent_id`). 0.6.1/0.6.2 added container memory caps + the Atropos flood gate — re-run `install.sh` or update your compose to pick up the 3g limits. |
| **v0.5.0** | Substrate | Typed-edge arsenal, ontology self-heal, layered `~/.helixir/helixir.toml` config, `helixir` CLI on PATH. |


## v0.3.x → v0.4.0 (the `algo_opt` read path)

**As of v0.4.0 the `algo_opt` profile is the DEFAULT.** Set
`HELIXIR_RETRIEVAL_PROFILE=legacy` to keep v0.3.x behaviour bit-for-bit.
Because the new default expects the new HQL queries on your instance,
existing installations should follow the steps below before (or right
after) updating the binary — until then, searches fall back to slower
legacy paths with a loud startup warning. To get the new read path (hybrid
BM25 search, bounded primary-key graph traversal, PPR ranking, provenance,
generation-LLM-free
chains, `connect_memories`), follow the steps below **in order**.

### 1. Update the binary

```bash
git pull && make build
```

Restart your MCP client afterwards (Claude Desktop / Cursor / Claude Code) —
**MCP clients cache the server binary and its env at session start**, so a
rebuilt binary or changed env vars do not reach the running server until the
client restarts.

### 2. Enable BM25 on your HelixDB instance

In the `helix.toml` that owns your instance, add to the instance section:

```toml
bm25 = true
```

Then redeploy the instance (rebuilds the container, data volume persists):

```bash
helix push <instance>     # or: make deploy-schema for the default setup
```

This also deploys the new HQL queries v0.4.0 needs
(`searchMemoriesByBm25`, `getConnectionsLevelBatch`,
`smartVectorSearchWithChunksCutoff`).

> **Archive your data volume first.** `make migrate-helix-fresh` shows the
> tar-based pattern; at minimum copy the instance's `.helix/.volumes/<name>`
> directory while the container is stopped.

### 3. Let the BM25 index build — then verify it

HelixDB builds its BM25 index **on insert**; for pre-existing data a full
rebuild runs automatically at container startup when the stored BM25
schema-version stamp differs from the binary's. Verify with a term you know
exists in your corpus:

```bash
curl -s -X POST http://localhost:<port>/searchMemoriesByBm25 \
  -H 'Content-Type: application/json' \
  -d '{"text":"<a word from your data>","limit":5}'
```

**If results are empty or partial** (possible when an older container had
already stamped the current schema version), force a clean rebuild: stop the
container, delete the `schema_version` key from the `bm25_metadata` database
inside the instance's LMDB (`.helix/.volumes/<name>/user`), start the
container — the rebuild runs on boot. A 50-line `heed3`-based helper for the
key deletion is described in `helixir/doc/v0.4.0-pre/notes.md`; pin
`heed3 = "=0.22.0"` / `lmdb-master3-sys = "=0.2.5"` to match HelixDB's LMDB
format, and note that a `lock.mdb` created inside a Linux container must be
moved aside before a macOS host process can write (restore it after).

### 4. Turn on the profile (and the optional accelerators)

Add to your MCP server env (e.g. `mcpServers.<name>.env` in the client
config):

```jsonc
"HELIXIR_RETRIEVAL_PROFILE": "algo_opt",
// optional but recommended:
"HELIXIR_EMBED_CACHE_PATH": "~/.cache/helixir/embed-cache.jsonl",
"HELIXIR_EMBED_CACHE_WARMUP": "1",   // pre-embeds your corpus once at startup
"HELIXIR_SELF_SEED": "1"             // Helixir seeds knowledge about itself
```

Restart the MCP client (see step 1).

### 5. Check the startup log

On boot with `algo_opt`, Helixir probes the instance for the required
queries and logs **one loud warning** listing anything missing, with the fix.
If you see `algo_opt deployment check: all required HQL queries present` —
you are done.

### Escape hatches

Each accelerator can be disabled independently without leaving `algo_opt`:

| Variable | Disables |
|---|---|
| `HELIXIR_DISABLE_NATIVE_BM25=1` | BM25 hybrid (vector-only phase 1) |
| `HELIXIR_DISABLE_BATCH_EXPANSION=1` | batched traversal (per-node legacy walk) |
| `HELIXIR_DISABLE_PPR=1` | PPR re-ranking (legacy combined scores) |

And `HELIXIR_RETRIEVAL_PROFILE=legacy` returns everything to v0.3.x
behaviour (unset now means `algo_opt`).

### Behavioural changes that are NOT gated by the profile

- **The decision engine can no longer delete memories.** A `DELETE` verdict
  is executed as `SUPERSEDE` (old fact stays in history); the intent is
  recorded and escalated. See `helixir/memory-charter.md` C1.
- `add_memory` responses may include a `needs_clarification` array (charter
  escalations). It is additive — clients that ignore it lose nothing.
