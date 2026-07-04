# Upgrading Helixir

> ⚠️ **Before ANY upgrade that touches HelixDB itself:** newer HelixDB builds
> default to **in-memory storage** — stopping the instance ERASES everything
> unless it runs with disk persistence (`helix start dev --disk`, or a mounted
> `HELIX_DATA_DIR` for containers as our compose/install configure). After the
> upgrade, verify: write a memory, restart the instance, confirm it survived.

## v0.4.x → v0.9.0 — all drop-in

Every release from v0.5.0 through v0.9.0 upgrades in place: update the
binary, restart your MCP client, done. New config keys are optional with
safe defaults. Version-by-version notes, newest first:

| Version | Theme | Worth knowing when upgrading |
|:--------|:------|:------------------------------|
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
