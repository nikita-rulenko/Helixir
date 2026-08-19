# Installation

> _Reflects code as of `v0.16.0`. Last verified: 2026-08-19._

This is the maintained installation reference. The root README intentionally
keeps only the shortest working path; topology choices, package trust,
headless operation, and lifecycle guarantees live here.

## Choose an installation path

| Path | Best for | What it installs |
|:-----|:---------|:-----------------|
| Homebrew | macOS and Linuxbrew | Signed native package and runtime resources |
| Helixir APT repository | Debian 12 and Ubuntu 22.04+ | Signed `amd64`/`arm64` Debian package and runtime resources |
| Release installer | Other supported hosts and scripted bootstrap | Matching immutable GitHub release archive |
| Source build | Contributors and unreleased branches | Locally compiled native binaries |

Every path ends at the same `helixir onboard` orchestrator. Package managers
do not provision Docker, HelixDB, models, MCP clients, RBAC, or the web control
plane from lifecycle hooks.

## Homebrew

```bash
brew install nikita-rulenko/tap/helixir
helixir onboard
helixir doctor --json
```

The fully-qualified formula automatically adds the maintained tap. It selects
an immutable release archive for macOS or Linux and the current architecture.

```bash
brew upgrade helixir
brew pin helixir
brew unpin helixir
brew uninstall helixir
```

Upgrade and uninstall affect package-owned files only. They preserve
`~/.helixir`, database volumes, models, configuration, backups, and MCP client
entries.

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

`--no-local-llm` skips only the optional fallback generation model. It does not
disable NLI or embeddings.

`helixir doctor` sends a real embedding probe. If the configured remote path is
missing or invalid, doctor reports the failure, offers or performs recovery
through Ollama plus Nomic, atomically updates the central configuration, and
verifies the repaired path.

## MCP clients

Onboarding detects and configures Codex, Claude Code, and Cursor. `helixir
setup` is the lightweight registration-only path and additionally supports
Claude Desktop and Gemini CLI.

Each client receives:

- the `helixir-local` MCP server entry;
- a stable lower-case `HELIXIR_RBAC_ACTOR`;
- the backend address;
- the canonical Helixir memory skill when the client supports skills.

Reasoning-provider and embedding credentials remain in the private central
configuration, not copied into every editor's JSON.

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

On macOS, the command normally starts with
`/Users/<you>/.helixir/current/helixir-mcp`.

## Prerequisites

- Docker for a managed local HelixDB and the optional control plane.
- Rust 1.88+ only when building from source.
- One reasoning LLM path: Cerebras, DeepSeek, Ollama, or another configured
  compatible endpoint.
- Helix CLI **v2.3.5** for source/schema development.

### HelixDB version pin

Helixir targets the v2/LMDB generation of HelixDB. HelixDB v3/hyperscale is a
different engine: it has no compatible `helix check` / `helix build` workflow
and does not register this repository's HQL schema. Never run `helix update` in
this project.

Install the exact upstream v2.3.5 artifact for the host. For example:

```bash
mkdir -p ~/.local/bin
curl -L -o ~/.local/bin/helix \
  https://github.com/HelixDB/helix-db/releases/download/v2.3.5/helix-x86_64-unknown-linux-gnu
chmod +x ~/.local/bin/helix
helix --version
```

Expected output includes `Helix CLI 2.3.5`. Other available artifact names are
`helix-aarch64-apple-darwin`, `helix-x86_64-apple-darwin`,
`helix-aarch64-unknown-linux-gnu`, and
`helix-x86_64-pc-windows-msvc.exe`.

A preserved mirror of the same release is available at
[nikita-rulenko/helix-db v2.3.5](https://github.com/nikita-rulenko/helix-db/releases/tag/v2.3.5).

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
