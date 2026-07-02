# v0.6.1 — the day-after patch

> _Supersedes `v0.6.0`. Everything the hive release's first day of real
> multi-agent traffic shook loose — shipped the same day._

## OOM containment (P0)

The HelixDB container was kernel-OOM-killed twice under normal load
(transient multi-GiB spike; DB itself only 32 MB). A/B load tests show no
steady leak — reads and writes both plateau — so until the spike's trigger
is pinned (#71), every deployment surface now runs the database CONTAINED:
`-m 3g --memory-swap 3g`, `--restart unless-stopped`, log rotation
(docker-compose, install.sh, the ops runbook). A runaway restarts in
seconds instead of taking the whole Docker VM down; LMDB is durable, so a
restart loses nothing — and the offending query survives in the logs.

## Release artifacts actually ship

The tag-triggered Release workflow had been silently red since v0.4.0:

- aarch64-linux: `vendored-tls` feature — the cross container has no
  target OpenSSL (helix-rs pulls reqwest with native-tls defaults)
- both macOS targets: lean-core artifacts (ort has no / flaky ONNX
  Runtime prebuilts on macOS runners — same posture as CI)
- Windows: unix-only daemon plumbing (setsid / pre_exec / libc::kill)
  now cfg-gated with honest foreground fallbacks
- the `helixir` CLI binary is finally IN the artifacts (the setup entry
  point had never shipped)
- Create Release is gated to tag refs, so workflow_dispatch validates
  builds without touching releases

All five platform artifacts attach to releases automatically again.

## FastThink: fast, honest, and survivable

- `think_commit` no longer re-runs LLM extraction over conclusions the
  session already holds: prepared atoms + SUPPORTS provenance edges +
  background entity enrichment. Measured 2.7 s vs the old 40–96 s; a
  novel conclusion commits with zero LLM calls (`fast_commit_e2e`).
- Provenance is SCOPED: only recalls in the conclusion's supporting
  subtree become SUPPORTS edges (a live probe caught ~105 edges of
  over-attribution from broad exploratory recalls).
- One-shot clients no longer lose sessions: the MCP server auto-saves
  every active session as `[INCOMPLETE]` on shutdown, recoverable via
  `search_incomplete_thoughts`.
- Prompts rewritten around an operational trigger ("the moment your plan
  is search-then-decide, do both inside a session") — verified by a
  blind zeroclaw probe that reached for FastThink unprompted and
  committed a provenance-carrying recommendation.

## Notices stop gaslighting the inbox

Duplicate notice rows (a surface_dispute race) + `markNoticeDelivered`
updating only the FIRST match made the same contradiction_review notices
re-deliver forever. The query now marks all matching rows; failures are
logged, ghosts purged, and the outbox drains to zero after one delivery.

## resolve_contradiction — the 20th tool

Contradiction review is now a first-class MCP action: `confirm` (my
memory stands), `retract` (the disputing memory supersedes mine, history
preserved), `preference` (both coexist). Non-destructive in every branch;
retired disputes stop re-surfacing. The category dictionary also got its
deferred cleanup (13 e2e-polluted categories dropped, 132 → 119).

## Capture at the milestone

The templates and cognitive protocol now trigger memory capture on the
EVENT (fix landed / test green / release shipped / decision made / dead
end proven), not "at the end of the step" — sessions get cut off, and a
postponed capture is a lost capture.
