# v0.6.2 — the flood gate

> _Supersedes `v0.6.1`. The OOM root cause, found and fixed._

The v0.6.1 investigation ended with containment; this release ends with
the culprit. An orphaned `helixir daemon run` (started during cadence
testing, never stopped) re-routed a slowly drifting corpus every 10
minutes for 9.5 hours. Each pass found near-identical insight threads —
one hop shifts, the content_key changes, a "new" hypothesis lands — and
persisted them as chunked memories: **53 passes, 173 near-duplicate
insights**, continuous chunk-embedding writes, and a working set that
ground the database container into the kernel OOM killer. No leak: the
moment the daemon stopped, memory froze flat.

## The flood gate (Atropos)

- **Subsume check against the graph**: before persisting, the new
  hypothesis's category set is compared with every insight already in
  memory — a re-routed sub-path of an existing lead is the SAME lead,
  not new knowledge. Skipped.
- **Per-pass cap**: at most `moira.atropos.max_persist_per_pass`
  (default 6) new hypotheses per pass, loudly logged when hit.
- Verified live: a fresh pass over the flooded corpus persisted 6
  genuinely-new insights and skipped 15 re-routes.

## Epilogue on observability

The rendezvous system had named the culprit all along: the daemon
heartbeats into the swarm roster as `daemon:<user>` — one
`swarm_status` call away. The ops lesson (check the roster when hunting
rogue load) is now seeded in the memory's own manual.
