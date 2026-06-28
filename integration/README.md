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
| [`SKILLS.md`](SKILLS.md) | Claude (Claude Code / Claude Desktop) as a reusable **Skill** | Copy to `~/.claude/skills/helixir-memory/SKILL.md` (rename to `SKILL.md`). It auto-triggers when memory is relevant. |

Both are self-contained and use the same use-case model — pick whichever fits
your agent stack (or both).

## Prerequisite: wire the Helixir MCP server

These specs assume your agent can reach the Helixir MCP tools
(`mcp__helixir-local__*` / `helixir-local__*`). Set it up once:

```bash
helixir setup     # interactive: writes the `helixir-local` MCP entry into
                  # Claude Code / Claude Desktop / Cursor / Gemini CLI configs
```

Or add it manually to your client's MCP config (stdio transport, the
`helixir-mcp` binary). See the repo README → **Integration**.

## Customize one thing: `user_id`

The templates use `claude` as the example `user_id`. Replace it with a stable id
for your agent or user — keep it **consistent** across sessions so the memory
stays coherent and personal search is scoped correctly. For a shared team
collective, give each agent its own `user_id` and use `scope="collective"` to
read across them.
