# Operations

> _Reflects code as of `v0.18.0`. Last verified: 2026-08-25._

This guide covers the native CLI, RBAC administration, configuration, gateway,
Moirai, Hygieia, the web control plane, and development operations. Installation
and package lifecycle are documented in [installation.md](installation.md).

## Operating model

HelixDB is the single source of truth for memories and RBAC. The native CLI,
MCP server, and global-admin web control plane are clients of the same graph
contracts; none owns a second user registry, ACL, dedup map, or memory cache of
record.

The main native binaries are:

| Binary | Purpose |
|:-------|:--------|
| `helixir` | Installation, RBAC, operations, Moirai, Hygieia, gateway, and configuration CLI |
| `helixir-mcp` | Native stdio MCP server used by agent clients |
| `helixir-deploy` | Version-pinned schema deployment helper |
| `helixir-client` | Thin remote-host bootstrapper; MCP gateway connection, onboarding admission, client registration and instruction doctor |

## Daily commands

```bash
helixir doctor --json                 # prove required runtime dependencies
helixir mode                          # solo | collective | insights capability tier
helixir rbac status --json            # inspect permanent graph policy
helixir config get                    # redacted effective configuration
helixir control-plane status          # browser UI and native supervisor
helixir model status                  # mandatory NLI readiness
helixir health                        # bounded Hygieia event journal
helixir journal                       # system activity journal
helixir insights                      # Moirai insight journal with provenance
```

Modes are capability tiers, not access-control profiles. Permanent RBAC applies
in `solo`, `collective`, and `insights`.

### Public CLI capability map

This is the canonical index of public top-level commands. Run `helixir <command>
--help` for flags. Hidden supervisor/apply bridges are implementation details
and are intentionally omitted.

| Command | Subcommands or main arguments | Capability |
|:--------|:------------------------------|:-----------|
| `config` | `get`, `set`, `edit`, `apply` | Inspect, mutate, validate and hot-apply layered configuration. |
| `setup` | `--target`, `--gateway`, `--mode`, `--dry-run` | Register MCP clients and install the canonical skill without full provisioning. |
| `onboard` | topology/model/RBAC choices, `--dry-run`, `--non-interactive` | Run the shared detect → prepare → apply → verify installation transaction. |
| `doctor` | `--json` | Prove database, NLI, embedding and runtime readiness; repair a broken embedding path through Ollama/Nomic with operator visibility. |
| `mode` | — | Explain the active `solo`, `collective`, or `insights` capability tier. |
| `rbac` | `bootstrap`, `status`, `migrate-teamleads`, `group`, `user`, `dedup`, `grant`, `revoke`, `show`, `check` | Administer graph-backed identity, roles, groups, federated dedup and permission checks. |
| `charter` | — | Show adopted learned rules and contradiction-precedent counts. |
| `swarm` | `--window` | Project the graph-backed agent roster and TTL-derived online state. |
| `heartbeat` | agent, role, host, status | Publish one explicit presence lease for a non-MCP worker or diagnostic. |
| `prune-agent` | agent id, `--yes` | Delete a genuinely junk Agent presence row; stale legitimate agents normally remain as provenance. |
| `categories` | `--limit` | Inspect the controlled category dictionary and member counts. |
| `clotho` | `seed`, `tag`, `grow` | Seed, apply and expand category tagging. |
| `lachesis` | `pmi`, `route` | Measure category overlap and route witness-backed cross-domain threads. |
| `chain` | user, topic, max hops | Reconstruct the longest coherent reasoning chain through a topic. |
| `atropos` | limits/route shape | Curate routed threads into ranked provenance-bearing hypotheses. |
| `pipeline` | user, thresholds/caps | Run Clotho → Lachesis → Atropos once. |
| `daemon` | `run`, `start`, `stop`, `status` | Schedule Moirai, merge and contradiction-reconciliation passes. |
| `journal` | `--tail` | Read recent agent/Moirai activity. |
| `insights` | `--tail` | Read the persisted Moirai hypothesis journal and witnesses. |
| `debt` | user, `--reconcile` | Inspect cross-owner contradiction debt and retire disputes that policy can settle. |
| `backfill` | `--limit` | Idempotently add missing scoped content fingerprints to older memories. |
| `merge` | `--limit`, `--threshold` | NLI-gated paraphrase convergence; contradictions are never merged. |
| `model` | `download`, `status`, `check`, `which` | Install and prove the mandatory host-specific local NLI judge. |
| `gateway` | `run`, `start`, `stop`, `status` | Share the MCP surface over streamable HTTP, optionally behind bearer auth. |
| `watch` | `run`, `start`, `stop`, `status`, `install`, `uninstall` | Run Hygieia once, detached, or as a login service. |
| `health` | `--tail` | Read Hygieia's bounded health and recovery journal. |
| `web` | bind/assets/token options | Launch the loopback browser surface directly for local operation or development. |
| `control-plane` | `install`, `status`, `uninstall` | Manage the hardened container plus reboot-safe native supervisor. |

Commands that mutate memory quality (`backfill`, `merge`, `debt --reconcile`),
policy, installation or recovery require the same global-admin authority as
their underlying Rust facade. CLI `heartbeat` is not a background liveness
claim. MCP sub-agents call `agent_heartbeat(actor_id, agent_id, status)` on start
and at progress boundaries without writing fake memory; one-shot instances use
`agent_farewell` when they finish.

## RBAC administration

The authenticated CLI principal comes from `HELIXIR_RBAC_ACTOR`. There is no
`--actor` flag that can spoof a different administrator.

### Bootstrap and status

```bash
export HELIXIR_RBAC_ACTOR=root

helixir rbac bootstrap \
  --operator root \
  --principal codex \
  --principal claude

helixir rbac status --json
```

Bootstrap is one-way, resumable, and idempotent. It creates:

- `default` for pre-RBAC memories and previously trusted principals;
- `onboarding` for newly discovered principals;
- membership-free `moirai` for global-admin-only generated hypotheses.

Only the explicit operator receives global `admin`. Trusted legacy peers receive
equal `groupadmin` access inside `default`, which recreates the historical
shared data plane without granting global control-plane privileges.

### Users and groups

```bash
helixir rbac user list --json
helixir rbac user show --user alice --json
helixir rbac show --user alice --json

helixir rbac group list --json
helixir rbac group create --id development --name "Development"

helixir rbac group add-user \
  --group development \
  --user alice \
  --role worker \
  --json

helixir rbac group remove-user \
  --group development \
  --user alice \
  --json

helixir rbac grant --user alice --role moderator --group development
helixir rbac revoke --user alice --role moderator --group development
helixir rbac group delete --id retired-project --yes
```

### Remote-client workspace onboarding playbook

`helixir-client connect` intentionally stops at the least-privileged
`onboarding/worker` grant. A global administrator completes placement on the
full Helixir host:

```bash
export HELIXIR_RBAC_ACTOR=root

helixir rbac user onboard \
  --user alice-laptop \
  --group development \
  --group-name "Development" \
  --description "Primary development workspace" \
  --role worker \
  --json
```

The command is a convergent server-side playbook:

1. require active permanent RBAC and a global-admin operator;
2. prove the principal has active or historical `onboarding`/`default`
   registration in HelixDB;
3. use the existing target group, or create a missing non-reserved group when
   `--group-name` is supplied;
4. grant `groupadmin`, `moderator`, `worker`, or `viewer` in that group;
5. remove active temporary `onboarding` roles unless `--keep-onboarding` is
   explicit;
6. reload policy and return the exact active roles, readable groups, own-write
   capability, and isolated/federated memory scope.

This ordering is interruption-safe: the working grant exists before temporary
access is removed, and a retry completes the same graph state without deleting
the User node or assignment history. `--group-name` is optional for an existing
group. The target may be reserved `default`, but never `onboarding` or `moirai`.
Use repeated runs for principals that intentionally belong to several working
groups; add `--keep-onboarding` only to the staged first run.

The global-admin control plane exposes the same operation under **Access graph
→ Onboarding**. Its inbox is derived from active `onboarding` assignments in
HelixDB, not from presence rows or a browser-local registry. Selecting a
working group and role grants and verifies the target scope before the server
removes temporary onboarding access.

Removing a user deactivates assignments but preserves the User node and role
history. Reserved workspaces cannot be deleted. The last global administrator
cannot be revoked.

Roles:

| Role | Authority |
|:-----|:----------|
| `admin` | Global memories, policy, reserved workspaces, Moirai, and web UI |
| `groupadmin` | Read/write and membership/role management in assigned non-reserved groups |
| `moderator` | Read/write assigned groups and group members' memories |
| `worker` | Read assigned groups; write only own authored memories |
| `viewer` | Read-only in assigned groups |

`teamlead` is retired legacy state. Convert existing assignments explicitly:

```bash
helixir rbac migrate-teamleads --yes
```

### Dedup federations

Groups deduplicate independently by default. A federation deliberately shares
deduplication and new-memory visibility across selected groups.

```bash
helixir rbac dedup create \
  --id engineering \
  --name "Engineering federation"

helixir rbac dedup list --json

helixir rbac dedup attach \
  --group development \
  --dedup-group engineering

helixir rbac dedup attach \
  --group platform \
  --dedup-group engineering

helixir rbac dedup detach \
  --group platform

helixir rbac dedup delete --id empty-federation --yes
```

Joining exposes existing federation history to the group. Detaching preserves
historical visibility but prevents future memories from receiving that group's
visibility edge. Never pass a dedup federation id as `group_id` on a memory
write; Helixir resolves federation membership server-side.

### Permission inspection

```bash
helixir rbac check --user alice --action read --owner bob
helixir rbac check --user alice --action write --owner alice
```

Normal MCP writes pass a stable `actor_id`, the memory owner as `user_id`, and
the concrete working `group_id`. Authorization fails closed when the principal,
group, or deployed policy query cannot be resolved.

## Memory stewardship and agent presence

The CLI exposes bounded maintenance views and explicit repair operations. They
operate on the same graph contracts as MCP; none bypasses RBAC or creates a
second registry.

```bash
helixir charter                         # learned rules + precedent counts
helixir swarm                           # TTL-filtered agent roster
helixir heartbeat --agent worker-1 \
  --role developer --status working     # one explicit lease
helixir chain --user Codex \
  --topic "release recovery"            # longest coherent path
helixir debt --user Codex               # unresolved contradiction debt
```

Each presence row is an execution instance owned by an explicit logical
`principal_id`. `swarm`/`swarm_status` preserve the instance roster for
diagnostics while aggregating online-agent counts by principal. `swarm` reports
terminal farewell states as offline immediately and hides
non-terminal agents after `swarm.presence_ttl_secs`. The Agent node remains
because it anchors `AGENT_CREATED` provenance. Use
`prune-agent --agent-id <id> --yes` only for true junk such as a renamed test
identity, not routine staleness.

Three global-admin repair commands are intentionally explicit:

```bash
helixir backfill --limit 100000          # add missing scoped fingerprints
helixir merge --limit 500 --threshold .85 # NLI-gated paraphrase convergence
helixir debt --user Codex --reconcile    # retire policy-settled disputes
```

`backfill` is idempotent. `merge` requires the mandatory local NLI judge and
never unifies contradictions. Reconciliation preserves preference diversity
and live factual disputes; it drains only debt that the current policy can
settle.

## Configuration

Effective configuration is layered in this order:

```text
built-in defaults < ~/.helixir/helixir.toml (or HELIXIR_CONFIG) < environment
```

Environment values win. Persistent secrets should normally live in the private
central configuration rather than duplicated MCP-client files.

```bash
helixir config get
helixir config get --raw
helixir config set <key> <value>
helixir config edit
helixir config apply
```

Secret-shaped fields (`*_key`, `*_token`, `*_password`, `*_secret`, and
`*_credential`) are redacted from resolved and raw output. The web Stewardship
room presents the same allowlisted configuration surface; secrets are
write-only and never returned to the browser.

`config apply` validates cross-field constraints, writes atomically, and
hot-reloads supported MCP/gateway processes. Daemon and watchdog instances that
hold deeper snapshots are reported as requiring a restart.

### Environment reference

| Variable | Default | Purpose |
|:---------|:--------|:--------|
| `HELIX_HOST` | `localhost` | HelixDB address |
| `HELIX_PORT` | `6969` | HelixDB port; a detected existing endpoint is preserved |
| `HELIXIR_RBAC_ACTOR` | — | Stable authenticated graph principal for this process |
| `HELIXIR_MODE` | `solo` | `solo`, `collective`, or `insights` capability tier |
| `HELIX_LLM_PROVIDER` | `cerebras` | Primary reasoning provider: `cerebras`, `deepseek`, or `ollama` |
| `HELIX_LLM_MODEL` | `gpt-oss-120b` | Primary reasoning model |
| `HELIX_LLM_API_KEY` | — | Remote primary-provider credential |
| `HELIX_LLM_BASE_URL` | provider default | Custom OpenAI-compatible or Ollama URL |
| `HELIX_LLM_FALLBACK_CHAIN` | empty | Explicit opt-in generation fallback tiers; the default write path remains Cerebras `gpt-oss-120b` only |
| `HELIX_DEEPSEEK_API_KEY` | — | DeepSeek fallback credential |
| `HELIX_EMBEDDING_PROVIDER` | `ollama` | `ollama` or OpenAI-compatible `openai` |
| `HELIX_EMBEDDING_URL` | `http://localhost:11434` | Embedding endpoint |
| `HELIX_EMBEDDING_MODEL` | `nomic-embed-text` | Embedding model |
| `HELIX_EMBEDDING_API_KEY` | — | Optional remote embedding credential |
| `HELIXIR_EMBED_CACHE_PATH` | — | Enable the private persistent embedding cache |
| `HELIXIR_EMBED_CACHE_MAX_BYTES` | `134217728` | Durable cache byte ceiling |
| `HELIXIR_EMBED_CACHE_EPOCH` | — | Explicit invalidation for opaque remote-model changes |
| `HELIXIR_EMBED_MODEL_REVISION` | auto for Ollama | Primary artifact revision override |
| `HELIXIR_EMBED_DIMENSION` | auto | Expected primary vector dimension |
| `HELIXIR_EMBED_FALLBACK_MODEL_REVISION` | auto for Ollama | Fallback artifact revision override |
| `HELIXIR_EMBED_FALLBACK_DIMENSION` | auto | Expected fallback vector dimension |
| `HELIXIR_EMBED_CACHE_WARMUP` | — | `1` for background or `blocking` for synchronous warmup |
| `HELIXIR_GATEWAY_TOKEN` | — | Bearer token for the network MCP gateway |
| `HELIXIR_GATEWAY_PUBLIC_URL` | — | Network-reachable `/mcp` URL advertised to administrators and remote clients |
| `RUST_LOG` | `helixir=warn` | Logging filter |

The persistent embedding namespace includes format, provider, normalized
endpoint, model, artifact revision, dimension, epoch, and a SHA-256 digest of
the exact input. Raw memory text is not written to the cache. Changing the
provider, endpoint, revision, dimension, or epoch produces safe misses without
touching HelixDB.

## MCP gateway

The gateway exposes the same tools through streamable HTTP from one long-lived
Helixir process per host. Prefer it for clients such as Codex whose isolated
tool sessions may retain abandoned stdio pipes: otherwise every retained pipe
keeps its `helixir-mcp` child alive even though the server correctly waits for
transport EOF.

```bash
helixir gateway start --bind 127.0.0.1:8765
helixir setup --gateway 127.0.0.1:8765
helixir gateway status
helixir gateway stop
```

On macOS and Linux, `start`, `status`, and `stop` manage a launchd or systemd
user service. The macOS LaunchAgent returns after login/reboot. The Linux user
unit returns with the user session; on a headless host that must serve before
login, the operator explicitly enables lingering with `loginctl enable-linger
<user>`. `start` replaces the service definition idempotently and retires any
legacy detached PID before binding the port; `stop` disables the service.
`status` exits non-zero when that reboot-safe owner is absent or unhealthy,
even if an unrelated foreground process happens to occupy the HTTP port.
`gateway run` remains the explicit foreground form. On unsupported platforms,
`start` falls back to the legacy detached-process lifecycle.
Managed services read the protected central config and reject transient
`HELIX_*`/`HELIXIR_*` overrides rather than silently losing them at the next
login. Persist configuration with `helixir config set`; secrets are never
copied into a plist or systemd unit.

`setup --gateway` uses the native Codex and Claude Code CLIs, backs up a
conflicting registration, replaces it with HTTP, verifies the result, and
restores the backup on failure. File-configured clients receive the equivalent
`type = http` / `url = .../mcp` entry. Restart each client after migration.

Stdio remains available for clients that cannot speak streamable HTTP. A stdio
server exits when its owning client closes stdin; an idle timeout is
intentionally not used because an otherwise quiet MCP session is still valid.

The default gateway bind is `0.0.0.0:8765` and assumes a trusted network. Enable
bearer authentication before exposing it beyond that boundary:

```bash
helixir config set gateway.auth_token <secret>
helixir config apply
helixir gateway start --require-auth
```

Listener coordinates and client coordinates are intentionally separate.
`gateway.default_bind` controls where the server listens; `gateway.public_url`
is the URL an administrator can safely send to another host. Configure it in
the Stewardship page, with `helixir config set gateway.public_url <url>`, or
through `HELIXIR_GATEWAY_PUBLIC_URL`. The Access graph shows the normalized
`/mcp` endpoint and a copy-ready client command. Wildcard and loopback binds are
shown for diagnosis but explicitly marked as not shareable.

`--require-auth` fails closed with `503` until a token is configured. Do not use
Helixir RBAC as a substitute for transport authentication against malicious
clients that can submit arbitrary `actor_id` values.

### Thin remote clients

`helixir-client connect` is intentionally not a second operations CLI. It can
normalize and validate one gateway URL, request its own bounded onboarding
admission, configure local agent clients, install canonical instructions and
write a non-secret profile. It cannot start HelixDB, install models, run the
Moirai or Hygieia, mutate group policy, manage backups, or open the admin UI.

```bash
helixir-client connect --gateway helixir-host:8765 \
  --principal cursor-workstation --owner cursor --project /work/project
helixir-client status
helixir-client doctor
```

For headless provisioning, keep the global profile flag before the subcommand
and make every mutation explicit:

```bash
helixir-client --profile /etc/helixir/client.json connect \
  --gateway https://helixir.example/mcp \
  --principal build-agent-01 --owner build-agent-01 \
  --project /work/repository \
  --client codex --client cursor \
  --token-env HELIXIR_GATEWAY_TOKEN \
  --yes --replace
```

`--client` is repeatable. `--token-env` stores only the environment-variable
name in the profile, never the bearer token itself. Omit `--replace` unless the
existing `helixir-local` registration is intentionally being superseded.

`connect` refuses to replace an existing conflicting `helixir-local` entry
unless `--replace` is explicit. Native Codex/Claude configuration and Cursor
JSON are backed up before replacement and verified afterwards. `doctor` is
client-scoped: it checks the MCP handshake/tool set, active graph-backed role,
local registrations, skill copy and managed `AGENTS.md`; it never probes or
repairs server-side NLI or embeddings.

## Moirai

The Moirai are background agents that compose normal graph primitives:

| Agent | Responsibility |
|:------|:---------------|
| Clotho | Grow the controlled category dictionary and tag memories |
| Lachesis | Route coherent cross-domain category paths with witnesses |
| Atropos | Curate routes into ranked, provenance-bearing hypotheses |

```bash
helixir categories
helixir clotho grow --user <id>
helixir lachesis route --seed <category>
helixir atropos
helixir pipeline --user <id>
helixir insights
```

The Moirai may analyze every working group, but only global admins can invoke
them or read the reserved `moirai` workspace. Persisted hypotheses use
`MOIRAI_DERIVED_FROM` witness edges, which ordinary reasoning traversal does
not cross. A hypothesis with no witness edge is an integrity failure.

### Background daemon

```bash
helixir daemon start --user <id> --interval 600
helixir daemon status
helixir daemon stop
```

Cadence can be set independently with `--clotho-every`, `--insight-every`,
`--merge-every`, and `--reconcile-every`. `0` disables that pass; `1` runs it on
every cycle; `N` runs every Nth cycle.

## Hygieia

Hygieia supervises database liveness, storage persistence, model readiness,
container memory pressure, orphaned daemons, and backup activity. Alerts and
recoveries are persisted as `ops_alert` outcomes for the configured operator.

```bash
helixir watch start
helixir watch run --once
helixir watch status
helixir watch stop
helixir watch install
helixir watch uninstall
helixir health
```

`watch install` creates a user-level launchd or systemd service on supported
macOS/Linux hosts. `watchdog.on_alert_cmd` can mirror each bounded alert to a
human-facing integration; alert kind and summary are passed through dedicated
environment values.

## Admin control plane

```bash
helixir control-plane install
helixir control-plane status
helixir control-plane uninstall
```

The browser surface binds to loopback, is available only to a graph-backed
global administrator, and combines:

- bounded memory, node, user, group, and live-agent projections;
- searchable RBAC and dedup-federation administration;
- a graph-derived onboarding inbox with atomic placement into working groups;
- a copy-ready advertised MCP gateway endpoint for remote client handoff;
- category-first graph exploration with drill-down into real memory nodes;
- Moirai evidence and Hygieia event journals;
- redacted settings with review-before-apply;
- resumable operation history;
- managed-database backup create, verify, and guarded restore.

The control-plane container is non-root, read-only, capability-free, and has no
Docker socket or host-home mount. Host mutations go through a narrow
token-authenticated native supervisor and typed allowlist.

### Backup vault

Managed backup operations are enabled only for a Helixir-managed local
database. Existing-local and remote installations remain observe-only because
their lifecycle belongs to another operator.

Restore requires the exact phrase `RESTORE <archive-id>`. The supervisor creates
a fresh safety snapshot, restores the selected archive, and probes the live
schema. An incompatible recovery automatically rolls back to the safety
snapshot and verifies the current schema again.

### Docker control-plane recovery without guessing about production

Treat Docker API failure and HelixDB data-plane failure as separate incidents.
If MCP/read-only memory calls still work while `docker info` or `docker inspect`
times out, stop every release, build and container gate: the database is still
serving, but Docker cannot safely prove container/volume state. Do not start a
second daemon, prune storage, delete volumes, or repeatedly issue mutations.

Recovery is an explicit operator action:

1. verify one production memory read and record the latest verified backup path
   and checksum;
2. stop all disposable gate activity and confirm no test targets port `6970`;
3. restart Docker Desktop from the host UI (or its documented host service
   action), accepting the short data-plane interruption;
4. wait for `docker info` to return normally, then inspect the production
   container: it must be running, use the expected image and volume, report
   `OOMKilled=false`, and show no unexpected restart loop;
5. verify the Helixir schema marker, permanent RBAC, read and one explicitly
   approved write through the product surface; compare memory count with the
   pre-restart observation;
6. remove only disposable resources by their exact gate labels, then rerun the
   isolated release gate from a clean target directory.

If the initial memory read fails, this is a production outage rather than a
Docker-control-only incident: stop immediately and recover through the managed
backup procedure above. A successful Docker restart is not release evidence;
the faithful 85-percent memory gate and full isolated E2E matrix must still pass.

## Development operations

```bash
make build
make check
make test
make run
make deploy-schema
make docker-up
make docker-down
```

The repository targets its maintained HelixDB v2.3.5 compiler fork. Before any
schema transition:

1. create and verify a recoverable backup of the persistent volume;
2. stop writers;
3. run `make build-helixdb-cli`, then `helix check` through that exact fork;
4. rebuild/recreate against the same persistent volume;
5. perform health and read-only query verification;
6. resume writers only after the schema contract is proven.

### Test gates

```bash
cargo fmt --all -- --check
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

Live E2E suites are ignored unless their explicit environment gates are set.
They must run against an appropriate disposable or backed-up live database;
never convert a flaky model assertion into a retry-until-green loop.

```bash
HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
  cargo test --test read_path_e2e -- --ignored --nocapture

HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
  cargo test --test mcp_read_e2e -- --ignored --nocapture
```

See [test-design.md](test-design.md) for the complete coverage map and
[UPGRADING.md](../../UPGRADING.md) for release-specific operational changes.
