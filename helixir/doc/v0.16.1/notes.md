# v0.16.1 — The One Gate

Released 2026-08-21.

Helixir v0.16.1 fixes a process-lifecycle failure observed with Codex code-mode.
Codex could retain abandoned stdio pipe writers after an isolated tool session,
so an otherwise correct `helixir-mcp` server never received EOF and sleeping
children accumulated.

## One process per host

- HTTP-capable clients can now share one explicitly managed streamable-HTTP
  gateway per host instead of spawning one stdio server per client session.
- `helixir setup --gateway <host:port>` now registers Codex through
  `codex mcp add --url` and Claude Code through its user-scoped HTTP transport.
- Cursor and other file-configured clients receive the equivalent HTTP MCP
  entry without backend, model, or provider credentials.

## Safe migration

- A conflicting native registration is backed up before replacement.
- The new transport is read back and verified; a failed removal, add, or
  verification restores the original client configuration.
- Registration comparisons normalize client-owned fields and recognize Codex's
  implicit HTTP transport shape.
- `helixir doctor` accepts both a verified installed stdio binary and a valid
  Helixir HTTP endpoint.

## Verification

- 336 library unit tests and 14 CLI tests pass.
- The complete Rust all-targets suite, including the 500-line module budget,
  passes.
- Strict Clippy passes with warnings denied.
- The control plane passes 11 unit tests, a production build, and 20 Playwright
  scenarios across Chromium, Firefox, WebKit, and mobile.
- A live post-restart Codex smoke completed three native memory recalls through
  the gateway with zero `helixir-mcp` children and exactly one gateway process.
- Live MCP initialize, `search_memory`, `add_memory`, and `doctor --json` all
  succeeded against the existing HelixDB/RBAC graph.

## Upgrade

There is no HelixDB schema or data migration from v0.16.0:

```bash
helixir gateway start --bind 127.0.0.1:8765
helixir setup --gateway 127.0.0.1:8765
helixir doctor --json
```

Restart Codex, Claude Code, Cursor, or another migrated client after setup so
it loads the HTTP registration and establishes a fresh gateway session.
