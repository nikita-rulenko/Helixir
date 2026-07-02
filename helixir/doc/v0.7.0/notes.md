# v0.7.0 — Hygieia

> _Supersedes `v0.6.2`. The organism learns to watch its own pulse._

Yesterday's OOM incident ended with containment (v0.6.1) and a root-cause
fix (v0.6.2). This release makes the lesson an organ: **Hygieia**, the
built-in health watchdog, joins the Moirai in the pantheon.

## Detectors

- **DB liveness** — the cheapest probe that exercises the full stack;
- **Container memory pressure** — docker stats against a % threshold;
- **Insight flood** — consecutive passes hitting the Atropos persist cap
  mean routing is re-finding the same drifting threads;
- **Orphaned daemons** — a daemon still heartbeating while every other
  agent has been silent past a horizon is probably forgotten (exactly how
  the incident started).

## The reaction ladder

1. **Self-heal silently.** A flooding insights stage is paused for the
   daemon's lifetime; a dead database container is restarted
   (`watchdog.allow_container_restart`, off by default; LMDB is durable —
   a restart loses nothing). The user never notices.
2. **Alert through the memory itself.** An `ops_alert` notice lands in
   every configured user's outbox — delivered in `pending_outcomes` on
   their next write, the same channel as contradiction reviews — plus a
   recallable `ops-alert` memory under `helixir`. Incidents become
   knowledge, not lost log lines.
3. **Journal everything.** Append-only `health.jsonl`; `helixir health`
   prints the tail.

## Hosts

Hygieia rides inside the Moirai daemon's pass loop, and runs standalone:

```
helixir watch start | run --once | stop | status
helixir health
```

`watch` survives a DEAD database by design — a failed initialize is her
first finding, not a fatal error.

## Configuration

```toml
[watchdog]
enabled = true
sample_interval_secs = 60
container_name = ""            # empty: memory detector + restart heal off
mem_alert_pct = 80.0
allow_container_restart = false
flood_passes_to_pause = 3
orphan_daemon_hours = 6.0
alert_users = ["helixir"]
alert_cooldown_secs = 21600
```

## Verified by killing the patient

`docker stop` on the live database → one watch tick → detected,
auto-restarted, `ops_alert` delivered to the outbox, heal + alert
journaled, next probe green. Unit tests cover the flood latch, docker
stats parsing and the orphan policy.
