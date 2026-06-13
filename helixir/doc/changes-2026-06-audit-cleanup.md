# Changes — codebase health, liveness oracle, cleanup (2026-06-13/14)

Branch `audit/codebase-health` → merged into `rw_buff` → `dev`. (`main` untouched;
a release lands there once the daemon track is done.) This is the material for
that future release note.

## Highlights

### Liveness oracle — new permanent test infrastructure
Two e2e suites that prove the product *behaviourally*, not just that it compiles:
- `tests/mcp_full_surface_e2e.rs` (**L1**) — drives **all 17 MCP tools** through
  the real stdio transport + write→read-back persistence.
- `tests/mcp_multi_consumer_e2e.rs` (**L2**) — the real topology (N MCP processes
  ↔ one HelixDB ↔ shared collective) and its 7 invariants: consensus, cross-user
  dedup, collective visibility, personal isolation, buffered multi-producer
  (none lost), outbox delivery, knowledge-never-deleted.
- `tests/concurrent_mcp_stress_e2e.rs` — concurrent collective/all stress guard.
- Harness: `McpClient::spawn_with_env` (per-consumer buffer ON/OFF).

Methodology established and used as the gate for every deletion: **"compiles" is
not proof of life — only the full MCP oracle + memory persistence is.**

### Dead-code cleanup — −4286 LOC (~15.6% of `src/`), oracle-gated
`src/` 27477 → 23191 LOC, removed in four stages, each verified by the oracle:
1. Tier-1 not-in-binary modules: `toolkit/analytics/`, `toolkit/misc_toolbox/`,
   `mind_toolbox/memory/{contradiction,relations,supersession,user_link,deletion,remark}`.
2. `core/services/{chunking,linking,resolution}` — dead twin (compiled, no caller).
3. `mind_toolbox/integrator/` — dead twin (was `mod`-commented).
4. `core/velocity/` + `core/exceptions.rs` (stale duplicate of `core/error.rs`).
`helpers/reserved.rs` and `core/levels/` deliberately KEPT (future / live).
Journal: `doc/codebase-audit.md`.

### Fixes
- **NaN-safe ranking (#41)** — `mind_toolbox::ranking::desc` (NaN sinks last) +
  `sanitize_unit` at the scoring boundary, applied to all 8 ranking sort sites.
  A NaN score used to panic `partial_cmp().unwrap()` and silently kill the MCP
  process. Latent hazard closed (an audit showed NaN is unreachable in the
  normal pipeline, so this is hardening — the real concurrent-agent crash is the
  multi-process resource model, see #42).
- **Collective discoverability (#40)** — the cognitive protocol / server
  instructions now state the memory is shared and tell an agent to widen an
  empty `personal` recall to `scope=collective`; added the missing tools to the
  prompt (connect_memories, list_memories, get_add_status, think utilities);
  fixed stale tool surfaces (check_inbox phantom, config tool list).
- **valid_from (#45)** — the schema default `valid_from: String DEFAULT
  "{{timestamp}}"` is a literal, not a macro (HelixDB's only timestamp default is
  `DEFAULT NOW`, valid on `Date` fields only). New additive query
  `addMemoryWithValidFrom` passes a real RFC3339 valid_from; zero-downtime
  (old `addMemory` kept). Forward-only — pre-existing rows keep the literal until
  an optional backfill. Verified live. Test: `tests/valid_from_e2e.rs`.

## Findings raised as issues (for the daemon track)
- **#42** — memory-provider daemon (one daemon per machine, many agents,
  local+remote). The central strategy; the stdio one-process-per-client model
  multiplies resources and breaks the ingest buffer's serial-worker guarantee.
- **#43** — cross-user consensus fragments under concurrent timing (HelixDB
  snapshot lag); visibility-gated writes consolidate, so it's timing not logic.
- **#44** — `add_memory`/`think_commit` return empty `memory_id` on dedup
  (can't tell saved vs linked vs failed).
- **#46** — charter raises false CONTRADICTS between unrelated facts.
- **#47** — longest-chain context reconstruction (narrate an event's evolution).
- **#39** — swarm awareness (collective discoverability + active-agents window).

## Operational notes
- bench@6970 redeployed (`helix push` from the heisenbug workspace) with
  `addMemoryWithValidFrom`; old `addMemory` retained for the old binary.
- The fix only reaches live writes once the MCP binary is rebuilt + the client
  rebooted (done 2026-06-14, binary built 00:50, verified live).
