# Installation

> _Reflects code as of `v0.18.0`. Last verified: 2026-08-25._

This is the maintained installation reference. The root README intentionally
keeps only the shortest working path; topology choices, package trust,
headless operation, and lifecycle guarantees live here.

## Choose an installation path

| Path | Best for | What it installs |
|:-----|:---------|:-----------------|
| Homebrew | macOS and Linuxbrew | Signed native package and runtime resources |
| Helixir APT repository | Debian 12 and Ubuntu 22.04+ | Signed `amd64`/`arm64` Debian package and runtime resources |
| Release installer | macOS/Linux scripted bootstrap without a package manager | Matching immutable GitHub release archive |
| Source build | Contributors and unreleased branches | Locally compiled native binaries |

Helixir has two explicit installation profiles. The full `helixir` package
ends at the `helixir onboard` orchestrator and may own the database, models,
gateway and control plane. The `helixir-client` package is a separate thin
remote-agent bootstrapper: it owns no server runtime and connects only to an
already configured MCP gateway. Package-manager lifecycle hooks are
non-interactive in both profiles.

The native release matrix also publishes Windows binaries, but the Bash
release installer and transactional host onboarding currently target macOS and
Linux. Windows users can inspect the archive, but should not treat it as a
supported one-command installation until the native Windows bootstrap is
implemented.

## Homebrew

```bash
brew install nikita-rulenko/tap/helixir
helixir onboard
helixir doctor --json
```

The fully-qualified formula automatically adds the maintained tap. It selects
an immutable release archive for macOS or Linux and the current architecture.

For an agent-only host, install the independent thin formula instead:

```bash
brew install nikita-rulenko/tap/helixir-client
helixir-client connect \
  --gateway helixir-host.example:8765 \
  --principal codex-laptop \
  --owner codex \
  --project "$PWD"
helixir-client doctor
```

The `helixir` and `helixir-client` formulae own disjoint executables and may be
installed together. The full formula remains an all-in-one local-agent host via
`helixir onboard`; it does not depend on the thin bootstrapper. The client
formula carries only `helixir-client`, the canonical skill, and integration
instructions, and depends on no server package.

```bash
brew upgrade helixir
brew upgrade helixir-client
brew pin helixir
brew unpin helixir
brew uninstall helixir
brew uninstall helixir-client
```

Upgrade and uninstall affect package-owned files only. They preserve
`~/.helixir`, database volumes, models, configuration, backups, and MCP client
entries.

Release qualification uses native Apple Silicon and Intel macOS virtual
machines. A "macOS Docker container" is not a valid substitute because Docker
Desktop hosts Linux containers in a Linux VM; it cannot exercise the Mach-O
binary, Homebrew prefix, or macOS filesystem lifecycle. Before a tag can be
published, both formulae install together from separate unpublished release
artifacts, pass `brew test`, reinstall, uninstall, and prove `~/.helixir` state
survives. The gate also proves that neither formula owns the other's binaries
or server-only runtime resources.

## Debian and Ubuntu

Supported package hosts are Debian 12 and Ubuntu 22.04 or newer on `amd64` and
`arm64`.

Install the dedicated keyring and repository definition:

```bash
curl -fsSL https://nikita-rulenko.github.io/Helixir/helixir-archive-keyring.gpg \
  | sudo tee /usr/share/keyrings/helixir-archive-keyring.gpg >/dev/null

echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/helixir-archive-keyring.gpg] https://nikita-rulenko.github.io/Helixir stable main" \
  | sudo tee /etc/apt/sources.list.d/helixir.list >/dev/null

sudo apt update
sudo apt install helixir
helixir onboard
helixir doctor --json
```

For an agent-only host, install the client package from the same repository:

```bash
sudo apt install helixir-client
helixir-client connect \
  --gateway helixir-host.example:8765 \
  --principal codex-laptop \
  --owner codex \
  --project "$PWD"
helixir-client doctor
```

`helixir-client` installs no HelixDB, Docker integration, NLI, Ollama, Nomic,
reasoning provider, Moirai, Hygieia, backup service, or admin UI. Those remain
server responsibilities. Its post-install hook only prints the next command.

The release gate also builds an isolated APT index and starts two clean Debian
client containers against one disposable gateway. Both clients connect at the
same time with different principal/owner identities; they then race the same
principal enrollment to prove it remains idempotent. A fresh HelixDB contract
separately proves group-scoped writes and reads for different owners and for
one owner assigned to two isolated groups. This gate makes client packaging,
admission, and memory visibility one release-blocking contract.

The archive-key fingerprint is:

```text
82AE 7735 0E9F DBF0 D7AF 8B58 E0A8 D062 DC6C 5161
```

Verify it before trusting a newly downloaded key. A replacement key must be
published beside the previous key for an overlap period before repository
metadata changes signers.

```bash
sudo apt-mark hold helixir
sudo apt-mark unhold helixir
sudo apt upgrade helixir
sudo apt remove helixir
```

Package maintainer scripts are non-interactive and do not silently start or
reconfigure external services. Removal preserves user-owned state and external
database volumes.

## Release installer

```bash
curl -fsSL https://raw.githubusercontent.com/nikita-rulenko/Helixir/main/install.sh | bash
```

The installer detects the platform, downloads the matching release archive,
verifies it, extracts it into `~/.helixir/versions/<version>`, and atomically
switches `~/.helixir/current`. It then launches guided onboarding.

Useful variants:

```bash
# Inspect the installer without applying host changes
curl -fsSL https://raw.githubusercontent.com/nikita-rulenko/Helixir/main/install.sh \
  | bash -s -- --dry-run

# Headless native installation without the browser control plane
curl -fsSL https://raw.githubusercontent.com/nikita-rulenko/Helixir/main/install.sh \
  | bash -s -- --no-web

# Unattended bootstrap after supplying every required choice
curl -fsSL https://raw.githubusercontent.com/nikita-rulenko/Helixir/main/install.sh \
  | bash -s -- --non-interactive
```

## Build from source

```bash
git clone https://github.com/nikita-rulenko/Helixir.git
cd Helixir

make build
make install
make doctor

# Or build/install only the remote-agent client
make build-client
make install-client CLIENT_ARGS='--gateway 10.0.0.12:8765 --principal codex-laptop'
```

`make build` compiles release binaries for the host. `make install` uses the
same versioned native layout and onboarding service as the packaged paths.

## What onboarding does

`helixir onboard` is a typed `detect → prepare → apply → verify` transaction.
The CLI and browser installer call the same Rust service; they do not maintain
separate mutation policy.

The resulting plan can:

1. discover platform, resources, installed clients, models, and database state;
2. select one of three HelixDB ownership contracts;
3. configure the primary reasoning LLM and a verified embedding path;
4. install and verify mandatory local NLI;
5. provision Ollama and `nomic-embed-text`, or validate an explicit remote
   embedding endpoint;
6. register supported MCP clients without replacing unrelated entries;
7. install the canonical Helixir Agent Skill;
8. bootstrap permanent graph-backed RBAC;
9. install the optional admin-only control plane;
10. run doctor and return a structured verification report.

Use `helixir onboard --dry-run` to inspect the exact plan before mutation.

## HelixDB topology choices

Onboarding distinguishes lifecycle ownership instead of guessing from a
reachable port.

### Managed local

Helixir creates and supervises the local HelixDB container, persistent volume,
schema lifecycle, backups, and health checks. This is the recommended default
for a new machine.

### Existing local

Helixir connects to a separately managed local database without taking over its
container, volume, or backup lifecycle. The endpoint and compatible deployed
schema must already exist.

### Remote

Helixir stores an explicit remote endpoint and treats database lifecycle as
external. The admin backup vault remains observe-only because a local
supervisor cannot safely snapshot or restore a remote volume.

The product default HelixDB port is `6969`. A detected compatible existing
endpoint is preserved rather than silently moved.

## Models and embeddings

### Mandatory NLI

Every supported build uses the local ONNX Natural Language Inference judge as
the contradiction and paraphrase safety boundary. Onboarding installs the
packaged model, verifies the ONNX Runtime ABI, and fails if inference is not
available.

```bash
helixir model download
helixir model status
```

### Embeddings

A verified embedding path is mandatory.

- The recommended local path installs/starts Ollama and pulls
  `nomic-embed-text`.
- A remote OpenAI-compatible provider is allowed only when provider, URL, and
  model are explicit; an API key is required when that endpoint authenticates.

```bash
# Fully local defaults
helixir onboard --non-interactive

# Keep a remote primary reasoning LLM but use local Nomic embeddings
helixir onboard --non-interactive --no-local-llm

# Explicit remote embeddings
HELIX_EMBEDDING_API_KEY=... helixir onboard --non-interactive \
  --remote-embeddings \
  --embedding-provider openai \
  --embedding-model text-embedding-3-small \
  --embedding-url https://api.openai.com/v1
```

The default does not install a generation fallback: memory extraction and
decisions stay on the configured Cerebras `gpt-oss-120b` path. Passing
`--local-llm-model <model>` is an explicit opt-in. `--no-local-llm` makes that
choice explicit and does not disable mandatory NLI or the selected embedding
runtime; local embeddings still install Ollama plus Nomic.

`helixir doctor` sends a real embedding probe. If the configured remote path is
missing or invalid, doctor reports the failure, offers or performs recovery
through Ollama plus Nomic, atomically updates the central configuration, and
verifies the repaired path.

## MCP clients

Onboarding detects and configures Codex, Claude Code, and Cursor. `helixir
setup` is the lightweight registration-only path and additionally supports
Claude Desktop and Gemini CLI. For HTTP-capable clients, prefer one managed
gateway per host so abandoned client sessions cannot retain separate stdio
children:

```bash
helixir gateway start --bind 127.0.0.1:8765
helixir setup --gateway 127.0.0.1:8765
```

On macOS, `gateway start` installs `com.helixir.gateway` as a launchd agent. On
Linux, it enables `helixir-gateway.service` through the user systemd instance.
Both variants start immediately and restart on failure. Launchd recovers after
login/reboot; the Linux unit recovers with the user session, while a headless
pre-login service requires `loginctl enable-linger <user>`. Re-running the
command is idempotent and first retires a legacy detached gateway so only one
process can own the endpoint. The gateway setup path safely backs up, replaces
and verifies existing native Codex/Claude registrations; it rolls the client
config back on failure.

Stdio clients receive:

- the `helixir-local` MCP server entry;
- a stable lower-case `HELIXIR_RBAC_ACTOR`;
- the backend address;
- the canonical Helixir memory skill when the client supports skills.

Reasoning-provider and embedding credentials remain in the private central
configuration, not copied into every editor's JSON.

HTTP clients receive only the gateway URL. Their MCP calls carry the stable
`actor_id`; the gateway owns backend and model configuration for the host.

### A remote agent on another host

Run a reachable gateway on the full Helixir host, then use the thin package on
each agent host:

```bash
# Helixir host (trusted subnet; add --require-auth outside it)
helixir gateway start --bind 0.0.0.0:8765

# Advertise the address remote agent hosts can actually reach.
helixir config set gateway.public_url http://10.0.0.12:8765/mcp

# Agent host (choose one package manager)
brew install nikita-rulenko/tap/helixir-client
# or, on Debian / Ubuntu:
sudo apt install helixir-client
helixir-client connect --gateway 10.0.0.12:8765 \
  --principal claude-laptop --owner claude --project /work/project
```

The endpoint is streamable HTTP at `/mcp`. It is not the HelixDB port. Connect
performs the MCP initialization handshake and refuses an incompatible gateway
before changing local files. A new principal can self-admit only as `worker`
in reserved `onboarding`; it cannot choose a role or group. Historical
admission is remembered, so reconnecting does not recreate revoked onboarding
access or downgrade roles an administrator assigned later.

Complete the principal's placement from the full Helixir host, never from the
agent-only machine:

```bash
export HELIXIR_RBAC_ACTOR=root
helixir rbac user onboard \
  --user claude-laptop \
  --group development \
  --group-name "Development" \
  --description "Product engineering workspace" \
  --role worker \
  --json
```

If `development` already exists, omit `--group-name` and `--description`. The
workflow verifies historical onboarding admission, creates a missing
non-reserved group, grants the requested role, removes the active temporary
`onboarding` grant, then reloads HelixDB policy and reports `readable_groups`,
write capability, and the effective group/dedup scope. It is safe to rerun
after interruption. Use `--keep-onboarding` only when the principal genuinely
needs both workspaces during a staged transition.

For every selected Codex, Claude Code, or Cursor client, the bootstrapper:

1. backs up and verifies the `helixir-local` HTTP registration;
2. installs the canonical `helixir-memory/SKILL.md` under the client-owned
   skill directory;
3. merges one marker-delimited block into the project `AGENTS.md`, preserving
   unrelated project rules;
4. stores a non-secret profile at `~/.helixir/client.json` with mode `0600`;
5. runs `helixir-client doctor` against gateway tools, RBAC admission,
   registrations and instruction freshness.

An optional bearer value is read from `HELIXIR_GATEWAY_TOKEN` by default and
is never written to the profile, MCP configuration, skill, or `AGENTS.md`.

For a custom stdio MCP client:

```json
{
  "mcpServers": {
    "helixir-local": {
      "command": "/home/you/.helixir/current/helixir-mcp",
      "env": {
        "HELIX_HOST": "localhost",
        "HELIX_PORT": "6969",
        "HELIXIR_RBAC_ACTOR": "codex"
      }
    }
  }
}
```

The same placement is available in the global-admin control plane under
**Access graph → Onboarding**. The page also displays the configured public MCP
endpoint and a copy-ready `helixir-client connect` command. Its pending list is
the live set of active `onboarding` memberships stored in HelixDB; it is not a
second user registry.

On macOS, the command normally starts with
`/Users/<you>/.helixir/current/helixir-mcp`.

## Prerequisites

- Docker for a managed local HelixDB and the optional control plane.
- Rust 1.88+ only when building from source.
- One reasoning LLM path: Cerebras, DeepSeek, Ollama, or another configured
  compatible endpoint.
- The checked-in maintained HelixDB v2.3.5 fork for source/schema development.

### HelixDB version pin

Helixir targets the v2/LMDB generation of HelixDB. HelixDB v3/hyperscale is a
different engine: it has no compatible `helix check` / `helix build` workflow
and does not register this repository's HQL schema. Never run `helix update` in
this project.

Release archives carry a server-only descriptor pinned to the immutable
multi-architecture maintained-backend image and its corresponding AGPL source
archive. Managed-local onboarding pulls and verifies that exact digest; remote
and existing-local backends are never replaced by the installer.

For source development, build the checked-in fork rather than installing an
upstream CLI:

```bash
make build-helixdb-cli
HELIX_REPO_PATH="$PWD/helixdb" ./helixdb/target/release/helix --version
make build-helixdb-image
```

Expected output is `Helix CLI 2.3.5`. The fork adds restart-safe secondary-index
backfill, bounded reader fan-out and collection-correct bulk updates without
changing the LMDB storage generation. `helixdb/UPSTREAM.md` records the exact
upstream revision and maintenance policy.

### Persistence warning

A database serving from in-memory storage loses its data when stopped. Managed
installations mount a persistent `HELIX_DATA_DIR`; CLI-managed development
instances must use the equivalent disk-backed mode. After any database
transition, verify persistence by writing a marker, restarting the instance,
and confirming the marker survived. Hygieia also reports
`storage_not_persistent` when a serving database has no LMDB files in its data
directory.

## Upgrade and removal guarantees

Package and archive upgrades replace immutable binaries and packaged resources
only. They preserve:

- `~/.helixir` and central configuration;
- HelixDB volumes and external databases;
- model files;
- managed backup archives;
- MCP client registrations;
- graph-backed RBAC, memories, and provenance.

Removing `helixir-client` likewise leaves `~/.helixir/client.json`, project
instructions, skill copies and agent-owned MCP configuration untouched; this
avoids a package purge silently mutating user workspaces.

After replacing binaries:

```bash
helixir onboard
helixir doctor --json
helixir control-plane status
```

Restart long-lived MCP clients so they load the new binary, tool schemas,
prompts, resources, and skill. See [UPGRADING.md](../../UPGRADING.md) for
version-specific transitions.

## Verification checklist

```bash
helixir --version
helixir doctor --json
helixir rbac status --json
helixir model status
helixir control-plane status
```

Doctor is successful only when required checks pass, including the real
embedding probe and mandatory NLI readiness. Do not validate installation by
writing a disposable test memory into a production graph.
