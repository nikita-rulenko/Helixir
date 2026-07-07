# v0.12.0 — The Operator Release

> _Alerts that reach a human, a watchdog that survives reboots, and a
> quieter, tidier repository underneath._

## Alerts reach outside the memory (#75)

Agents hear Hygieia's ops alerts through the memory — but a human who is
not currently talking to an agent heard nothing. New hook:

```toml
[watchdog]
on_alert_cmd = 'osascript -e "display notification \"$HELIXIR_ALERT_SUMMARY\" with title \"Helixir: $HELIXIR_ALERT_KIND\""'
```

Every alert fires the command (fire-and-forget, via `sh -c`) with
`HELIXIR_ALERT_KIND` and `HELIXIR_ALERT_SUMMARY` in the environment.
Point it at a desktop notification, a webhook, a pager — anything. A hook
failure is logged and never blocks the alert path.

## The watchdog becomes a service (#75)

`helixir watch` only lived as long as its terminal. Now:

```
helixir watch install     # launchd agent (macOS) / systemd user unit (Linux)
helixir watch uninstall
```

The installer refuses binaries under `target/` — a service pinned to a
build directory dies on the next `cargo clean`. Promote the binary first
(`~/.helixir/bin`), then install.

## FastThink recall leaves room to conclude (#78)

A recall that filled the session to its thought cap left no headroom to
synthesize — the session could gather evidence and then be unable to say
what it meant. Recall now stops `fast_think.conclude_reserve` (2) thoughts
short of the cap, so synthesis and `think_conclude` always fit.
`think_conclude` itself works even at 0 headroom, as before.

## The HelixDB version pin — read this if you self-host

`install.helix-db.com` now ships CLI **v3.x — a different, incompatible
database** (LSM over object storage; no `helix check`/`build`; our schema
never registers, the gateway answers with `query_count: 0`). Helixir targets
the v2/LMDB generation: **CLI v2.3.5**, pinned in the README Prerequisites
with the exact per-platform install command. Never `helix update`. A
preserved mirror of the v2.3.5 source (`v2-lts` branch) and all five CLI
binaries lives at
[nikita-rulenko/helix-db](https://github.com/nikita-rulenko/helix-db/releases/tag/v2.3.5),
in case upstream ever drops v2 artifacts. (Field-reported by a fresh
Windows/WSL2 install — thank you.)

## Agent prompts: full surface coverage

The integration templates (`integration/AGENTS.md`, `SKILLS.md`) now
enumerate the complete toolbox — `update_memory`, `get_memory_graph`,
`search_incomplete_thoughts`, `get_add_status` were missing — and all 8
ontology types in the write-for-ontology guidance (fact and action were
implied, now explicit). If you deployed the templates before, re-copy them.

## Housekeeping (#15) and one rename (#9)

- Tests no longer mutate process env via `unsafe set_var` — scoped
  `temp-env` everywhere; no cross-test races.
- `helixir-deploy` is a real clap CLI: `-h` is `--help`, `--version`
  exists, and an invalid `--port` is an error instead of a silent default.
- Default logs are pure ASCII — emoji stripped from every `tracing` line.
  (CLI cosmetics in interactive binaries are unchanged.)
- `.snapshots/` is gitignored; 13 ad-hoc ansible fix playbooks archived
  out of the tree; the deployment policy is written down in AGENTS.md.
- `smart_traversal_v2` is now `smart_traversal` — the `_v2` suffix
  outlived the twin it was named against.
- `helixir --version` finally answers (the update playbooks ask it).

## Upgrading

Drop-in: update the binary, restart your MCP client. No schema changes,
no new required config. New optional knobs: `watchdog.on_alert_cmd` (off
when empty) and `fast_think.conclude_reserve` (2).
