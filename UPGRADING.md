# Upgrading Helixir

> ⚠️ **HelixDB version pin: CLI v2.3.5.** Helixir targets the v2 (LMDB)
> generation of HelixDB; **v3.x is incompatible** (different engine, no
> `helix check`/`helix build`, schema never registers — `query_count: 0`).
> Every `helix …` command in this file assumes CLI 2.3.5 — see the README
> Prerequisites for the pinned install command. Do NOT `helix update`.

> ⚠️ **Before ANY upgrade that touches HelixDB itself:** newer HelixDB builds
> default to **in-memory storage** — stopping the instance ERASES everything
> unless it runs with disk persistence (`helix start dev --disk`, or a mounted
> `HELIX_DATA_DIR` for containers as our compose/install configure). After the
> upgrade, verify: write a memory, restart the instance, confirm it survived.

## v0.4.x → v0.13.1 — all drop-in

Every release from v0.5.0 through v0.13.1 upgrades in place: update the
binary, restart your MCP client, done. New config keys are optional with
safe defaults. Version-by-version notes, newest first:

| Version | Theme | Worth knowing when upgrading |
|:--------|:------|:------------------------------|
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
BM25 search, batched graph traversal, PPR ranking, provenance, LLM-free
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
