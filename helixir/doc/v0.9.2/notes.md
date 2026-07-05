# v0.9.2 — Flashbacks

> _Recall a period; associate beyond it. The window bounds attention — the
> graph reaches through it, honestly labelled._

The amplifier thesis, applied to time. An agent can now ask the memory
"what happened in June?" and get June — plus the out-of-June memories the
graph knows are *linked* to June, each one flagged and dated, the way a
human thinking about last week suddenly remembers last year and knows
it's old.

## Time windows & flashbacks (#87)

- `search_memory` takes `time_from` / `time_to` (RFC3339 or `YYYY-MM-DD`,
  either side may be open) — an explicit window on EVENT time
  (`valid_from`, else `created_at`). `temporal_days` is now the one-sided
  shorthand for the same machinery.
- The window hard-filters **seeds only**. Graph expansion is exempt: an
  out-of-window memory linked to an in-window result returns as a
  **flashback** — `metadata.flashback: true` + `event_date`, capped by a
  separate allowance (`retrieval.flashback_max`, default 3) so
  associations never crowd the period's own rows.
- Malformed bounds are rejected loudly; an inverted window is an error,
  not an empty result.
- Prompts, tool descriptions, AGENTS/SKILLS teach the reading rule with
  worked examples: a flashback is presented dated ("related, from
  2025-05: …"), never as an event of the requested period.

## The read path got honest about cost (#88)

On a dense graph, expansion once turned 9 seeds into 1710 rows — and the
real-cosine rerank embedded every one of them to finally keep 5–20.
Full-mode searches took tens of seconds and read as hangs. Now the rerank
embeds only the top `retrieval.rerank_max_rows` (128) candidates by
pre-rerank score, logs the truncation, and leaves the tail fully
reachable through PPR. Measured live: the same query went from
never-answering-in-45s to **2.4s**, identical results.

## FastThink stops starving weak models (#90)

The #81 recall belt (score floor 0.6, 30-day mode) had a silent-zero
failure mode: evidence that plain search finds was invisible inside
`think_recall`. A strong model sharpens its query; a weak one concludes
"no evidence exists" and reasons unsupported. Now a zero-result primary
pass triggers ONE fallback pass — whole store, relaxed floor
(`fast_think.recall_fallback_min_score`, 0.45), hard cap
(`recall_fallback_max`, 3) — and every fallback row is annotated
`[weak recall, score …]` in the thought itself, so provenance stays
honest about evidence quality. The belt is intact: the fallback cap is
smaller than the primary in every preset, unit-enforced.

## Hygieia: the cache valve (#89)

The scary `docker stats` number (2.58GiB on a 60MB store) turned out to
be reclaimable page cache, not heap — true working set was ~414MiB. Two
new levers:

- **Automatic**: the memory detector now opens cgroup `memory.reclaim`
  FIRST; only pressure that survives the shed alerts as real
  `mem_pressure`. Opt-in (`watchdog.allow_cache_reclaim` — it spawns a
  short-lived privileged helper, cgroupfs is read-only in-container);
  step `watchdog.reclaim_step_mib` (1024). A successful shed journals
  `heal/cache_reclaimed`.
- **Manual**: `make mem-probe` profiles where the memory actually goes
  (reconciles docker stats vs cgroup vs /proc, classifies mappings);
  `make mem-reclaim` sheds cache on demand. Judge the container by
  memprobe, not by docker stats.

## Also in this release

- **#36**: graph beam width honors
  `retrieval.graph.expansion_children_per_parent` — the last hardcoded
  read knob is gone; depth traces verified honest level by level.
- **#13**: deploy surfaces describe reality — `docker-compose.yml`,
  `install.sh` and the Makefile now build the image via the HelixDB CLI
  (the `helixdb/helixdb` Hub image never existed) and configure disk
  persistence explicitly.
- New project mark (the vacuum-tube brain).

## Upgrade

Drop-in from any v0.9.x: replace the binary, restart your MCP client
(the client caches tool schemas — `time_from`/`time_to` appear after the
restart). All new config keys are optional with safe defaults; the cache
valve is off unless you opt in. If you deploy with the old compose file,
re-run `install.sh` or pull the new one — it now describes an image that
exists.
