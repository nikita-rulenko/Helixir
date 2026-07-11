# v0.13.1 — The Honest Valve

One fix, found by heap forensics on the live container (#89).

## The cache valve now asks for everything

The memory HelixDB "holds" is two layers: lazily-freed pages the
allocator returns on request, and a working cache that only a restart
resets. The valve (cgroup `memory.reclaim`) could always shed the first
layer — but it asked for a fixed 1024MiB per opening. On a 3GB-charged
container that under-asks by 3x, and the leftover produced a false
"persists after reclaim, this is live heap" verdict — which then
triggered restarts that were not yet needed.

Hygieia's valve and `tools/memprobe.py --reclaim` now request the FULL
current charge (`reclaim_step_mib` remains as a floor for tiny
containers). Verified live: a lab container's lazy layer shed from
365MB to 0.4MB on one full ask; the post-reclaim number is now an
honest live-heap figure, so `mem_restart_pct` fires only when a restart
is genuinely due.

## Upgrading

Drop-in: replace the binary, restart `helixir watch` (a running
watchdog keeps its old valve until restarted). No config or schema
changes.
