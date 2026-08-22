# Integration templates — give your agents Helixir memory

Drop-in specs so **any** AI agent in **your** project uses Helixir the way it's
meant to be used — recall before answering, capture durable facts, reason in a
persistent scratchpad, trace the *why* behind decisions. These are the same
rules the Helixir maintainers run; copy them and your agents get the same
quality.

## What's here

| File | For | How to use |
|---|---|---|
| [`AGENTS.md`](AGENTS.md) | Any coding agent (Cursor, Claude Code, Codex, Aider, Continue, …) via the [agents.md](https://agents.md) convention | Copy to your **project root** as `AGENTS.md` (or merge into your existing one). Most agents read it automatically. |
| [`SKILLS.md`](SKILLS.md) | Legacy portable copy of the reusable **Skill** | Copy only when the client cannot consume the canonical [`helixir/skills/helixir-memory/SKILL.md`](../helixir/skills/helixir-memory/SKILL.md). |

Both are self-contained. The versioned canonical skill in `helixir/skills/`
is what `helixir onboard` installs for Codex, Claude Code and Cursor; keep a
manual copy only for clients the orchestrator cannot manage.

## Prerequisite: install and wire Helixir

From v0.16.0 onward, install the release through Homebrew or the signed APT
repository, then run the same guided orchestrator on every platform:

```bash
brew install nikita-rulenko/tap/helixir  # macOS / Linuxbrew
# or: sudo apt install helixir           # after adding the repository from README.md

helixir onboard
```

`onboard` discovers supported clients, provisions mandatory NLI plus a verified
embedding path, converges permanent RBAC and installs the canonical memory
skill. The release-archive and source-build paths in the root README call the
same orchestrator.

These specs assume your agent can reach the Helixir MCP tools
(`mcp__helixir-local__*` / `helixir-local__*`). Set it up once:

```bash
helixir setup     # interactive: writes the `helixir-local` MCP entry into
                  # Claude Code / Claude Desktop / Cursor / Gemini CLI configs
```

`setup` is the lightweight MCP-registration-only path; prefer `onboard` for a
new machine.

For an agent on another host, install only the thin package and point it at an
already running Helixir gateway:

```bash
# macOS / Linuxbrew
brew install nikita-rulenko/tap/helixir-client

# or Debian / Ubuntu after adding the signed repository
sudo apt install helixir-client

helixir-client connect --gateway helixir-host:8765 \
  --principal codex-laptop --owner codex --project "$PWD"
helixir-client doctor
```

This path installs the canonical skill and a managed `AGENTS.md` block, but no
database, NLI, Ollama/Nomic, daemon, Moirai, Hygieia, backup service or UI. A
new principal is admitted only as `worker` in reserved `onboarding`; an admin
assigns normal working groups later. The endpoint is `/mcp` on the gateway,
never the HelixDB database port.

Or add it manually to your client's MCP config (stdio transport, the
`helixir-mcp` binary). See [Installation and onboarding](../helixir/doc/installation.md)
for the full standalone and remote-client flows.

## Customize identity and access

The templates use `claude` as both example values. Replace it with the stable
principal provisioned by onboarding and pass that lower-case value as
`actor_id` on every call. Keep a stable `user_id` for memory ownership; it may
equal the actor, but it is provenance rather than an authorization credential.
Working-group writes also name their concrete `group_id`. `scope="collective"`
can rank other owners only inside groups the actor is already allowed to read.
