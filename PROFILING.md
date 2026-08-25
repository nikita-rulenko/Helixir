# Helixir profiling manifest

> Status: release-blocking diagnostic contract for [#168](https://github.com/nikita-rulenko/Helixir/issues/168),
> [#169](https://github.com/nikita-rulenko/Helixir/issues/169), and
> [#170](https://github.com/nikita-rulenko/Helixir/issues/170).
> The canonical datastore remains HelixDB v2.3.5 until differential evidence
> justifies a separately licensed fork. Last verified: 2026-08-23.

This manifest defines how Helixir memory incidents are reproduced, profiled,
attributed, and accepted. Its purpose is to answer two different questions
without conflating them:

1. **Which process owns the growth?** The differential `helixdb-mock` gate
   compares the same workload against a bounded emulator and real HelixDB.
2. **Which allocation path owns the bytes?** CPU and heap profilers resolve the
   Rust stacks after the process boundary is known.

Profiling is evidence, not a substitute for the release workload. A profiler
changes timing and may change the allocator; therefore only an uninstrumented
faithful run may pass or fail the release memory gate.

## 1. Non-negotiable rules

- Never profile the production HelixDB on port `6970` or the production Docker
  daemon. Use a disposable loopback Docker daemon and a cold, checksummed copy.
- Keep the Helixir workload, `helixdb-mock` or HelixDB, and the sampler in
  separate processes/cgroups. Report their memory independently.
- Independent hard limits are mandatory for the database and workload. Abort
  either process at **85 percent** of its own limit, before the kernel can
  deliver an OOM kill; never let one component borrow the other's headroom.
- Start every scenario from an identical cold datastore state and cleanly
  restart it afterward. Any `OOMKilled`, restart, stale image id, or changed
  cold-copy checksum invalidates the run.
- Never record request bodies, memory text, credentials, API keys, bearer
  tokens, embeddings, core files, or heap dumps in git or public CI artifacts.
- Store private artifacts below `~/.helixir/profiles/<run-id>/` with directory
  mode `0700` and file mode `0600`. Public reports contain only redacted
  aggregates and SHA-256 checksums.
- Never treat a diagnostic allocator build as canonical. An allocator change
  can hide fragmentation, retention, or the original defect.
- A mock run is a contract and attribution oracle, not a substitute for the
  live HelixDB compatibility gate.

## 2. Two lanes

### 2.1 Faithful lane — canonical release evidence

The faithful lane preserves production behavior:

| Target | Binary and allocator | Instrumentation |
|---|---|---|
| Helixir | release candidate, Rust system allocator | cgroup/RSS/smaps and external CPU sampling only |
| `helixdb-mock` | default release binary, no profiling allocator | cgroup/RSS/smaps and request-shape metrics |
| HelixDB | pinned v2.3.5 image, original mimalloc configuration | cgroup v2, RSS/smaps, `perf` sampling, mimalloc runtime statistics when available |

The faithful result alone answers whether the 3 GiB envelope is respected.
Profiler overhead must not be present in the baseline measurement used for the
release verdict.

### 2.2 Diagnostic lane — allocation-stack evidence

Run the diagnostic lane only after a faithful reproduction has captured the
same immutable trace digest:

| Target | Preferred tools | What they answer |
|---|---|---|
| Native macOS Helixir | Xcode Instruments `Allocations`, `Leaks`, `VM Tracker`; `samply` | allocation lifetime, VM growth, CPU stacks |
| Linux/Docker Helixir | `samply` or `perf`/flamegraph; `heaptrack`; optional `dhat-rs` build | CPU stacks, retained allocations, peak/live heap |
| `helixdb-mock` | `samply`/`perf`; `heaptrack`; optional `dhat-rs` build | whether the emulator or Helixir harness leaks independently |
| Faithful HelixDB v2.3.5 | `perf`; cgroup/smaps; `MIMALLOC_SHOW_STATS=1` in a compatible debug build | hot query stacks and allocator high-water behavior without changing allocator family |
| Diagnostic HelixDB fork | debug symbols plus `heaptrack`, or a separately labelled jemalloc profiling build exported to pprof | exact allocation call stacks when mimalloc statistics are insufficient |

Official references:

- [Rust Performance Book: profiling](https://nnethercote.github.io/perf-book/profiling.html)
- [Apple: analyzing heap memory with Instruments](https://developer.apple.com/videos/play/wwdc2024/10173)
- [jemalloc profiling controls](https://jemalloc.net/jemalloc.3.html)
- [mimalloc runtime and debug statistics](https://github.com/microsoft/mimalloc/blob/main/readme.md)

## 3. Build contract

Every profiled Rust binary must retain resolvable symbols while preserving
release optimization. Use the repository's `profiling` Cargo profile where it
exists; otherwise the equivalent requirements are:

```toml
[profile.profiling]
inherits = "release"
debug = "line-tables-only"
strip = "none"
```

Record the exact compiler, target, commit, binary SHA-256, image id, build
profile, allocator, and profiler versions. Do not silently profile a locally
modified binary under the release candidate's name.

An optional `dhat-rs` or jemalloc feature must be disabled by default and must
produce a visibly different binary/profile label. It may identify stacks, but
it cannot produce the release verdict.

## 4. Immutable run identity

Every run and artifact uses one `run_id` and one `trace_digest`. The report must
record at least:

```json
{
  "run_id": "20260823T153000Z-6269b1c-real-faithful",
  "scenario": "daemon-on-call",
  "trace_digest": "sha256:...",
  "target_kind": "real_helixdb",
  "git_commit": "6269b1c",
  "binary_sha256": "...",
  "container_image_id": "sha256:...",
  "build_profile": "release",
  "allocator": "mimalloc",
  "profiler": "none",
  "database_memory_limit_bytes": 3221225472,
  "workload_memory_limit_bytes": 1073741824,
  "abort_fraction": 0.85
}
```

The request trace records query name, parameter **keys**, status, duration,
response byte count, response shape, and response hash. It never records raw
parameter values or response content.

## 5. Required flow

### Step 0 — protect production

1. Verify the production Helixir MCP can read memory.
2. Verify the production HelixDB has `OOMKilled=false` and no unexpected
   restart count.
3. Confirm the investigation uses a disposable loopback Docker daemon and no
   target exposes host port `6970`.
4. Verify the cold-copy checksums and exact candidate image/binary ids.
5. Stop immediately and alert the operator if production Helixir is unavailable.

### Step 1 — validate the experiment without running it

The harness accepts a versioned JSON trace and supports a non-mutating dry run:

```bash
python3 tools/memory_boundary.py \
  --trace /private/path/daemon-memory-trace.json \
  --dry-run
```

Dry-run must reject production port `6970`, a non-loopback Docker endpoint,
missing image ids, missing cold-copy hashes, secret-shaped environment fields,
an abort fraction above `0.85`, or a shared workload/database container.
Faithful real-database scenarios that select a partial `HELIXIR_CONFIG` must
also declare `llm_runtime` pinned to `cerebras` / `gpt-oss-120b`. The key never
belongs in the trace: point `HELIXIR_MEMORY_HARNESS_LLM_CONFIG` at a private
`0600` base TOML outside the repository. The harness copies only the in-memory
credential into the child environment, does not serialize it, and fails closed
when the source is missing, empty, permissive, or configured for another model.

### Step 2 — establish unprofiled faithful baselines

1. Run the exact daemon trace against `helixdb-mock`.
2. Reset all state.
3. Run the identical trace against a cold HelixDB v2.3.5 copy.
4. Capture database and workload RSS, anonymous/file/swap memory, cgroup peak,
   cgroup events, exit status, restart count, and query cadence.
5. Abort with a distinct `database_memory_guard` or `workload_memory_guard`
   when either component reaches 85 percent of its own hard limit. Cleanly
   terminate the workload and reset the database even after failure.
6. Repeat a suspicious result at least once before assigning the root cause.

The live command is intentionally guarded and writes only outside the repo:

```bash
HELIXIR_MEMORY_HARNESS_DISPOSABLE_DOCKER=1 \
HELIXIR_MEMORY_HARNESS_LLM_CONFIG="$HOME/.helixir/helixir.toml" \
python3 tools/memory_boundary.py \
  --trace /private/path/daemon-memory-trace.json \
  --output "$HOME/.helixir/profiles/<run-id>"
```

### Step 3 — classify the process boundary

| Observation | Initial classification |
|---|---|
| Helixir RSS grows against bounded mock while mock remains stable | Helixir live-object/cache/task growth |
| Helixir and mock remain bounded; real HelixDB anonymous memory grows | HelixDB engine/allocator retention |
| Mock grows with its bounded-state limit intact | Invalid mock or harness implementation; fix before inference |
| Both isolated sides are stable, but the real pair grows | Interaction/query amplification; correlate cadence and payload size |
| Only file memory grows and drops after reclaim | Page cache, not a Rust heap leak |

This classification selects the profiler target. Do not profile every component
at once; that destroys attribution and adds unnecessary overhead.

### Step 4 — capture CPU stacks

Capture one CPU profile using the same trace digest. CPU profiles reveal hot
query paths, retry loops, repeated scans, serialization, and allocator work.

- macOS native process: `samply record <binary> ...` or Xcode Instruments.
- Linux/container process: `perf record`/flamegraph or `samply`, with the
  minimum ptrace/perf capability on the disposable environment only.

The profile must have resolved Rust symbols. An unresolved profile is not an
accepted artifact.

### Step 5 — capture heap allocation evidence

Choose one heap profiler for the already identified process:

- `heaptrack` for Linux allocation stacks where the allocator is observable;
- `dhat-rs` for a separately labelled diagnostic Helixir or mock build;
- Instruments `Allocations`/`Leaks` for native macOS Helixir;
- mimalloc statistics for the faithful HelixDB allocator;
- jemalloc pprof only for a separately labelled diagnostic HelixDB fork build.

Take comparable snapshots before the workload, near the same query boundary,
and immediately before the 85 percent abort. Never wait for an actual OOM to
obtain a final dump.

### Step 6 — correlate and decide

Join profiler timestamps with the redacted request trace and stage markers.
Classify the evidence:

| Heap evidence | Meaning |
|---|---|
| Live bytes and object count grow from the same Rust stacks | application-level retention or unbounded collection |
| Live heap is bounded but RSS/committed pages grow | allocator retention or fragmentation |
| One HQL/SearchV stack dominates allocation and growth | HelixDB query-engine defect or an abusive query shape |
| Request count/payload grows while per-request heap is released | Helixir query amplification; bound/batch/cancel the workload |
| Growth disappears only after allocator substitution | allocator-sensitive symptom; faithful run remains authoritative |

Persist one concise conclusion with links to the private artifact checksums and
the exact trace digest. A conclusion without a faithful reproduction and
resolved stacks remains a hypothesis.

## 6. Artifact layout

```text
~/.helixir/profiles/<run-id>/
├── metadata.json          # run identity; no secrets
├── report.json            # redacted verdict and peaks
├── samples.tsv            # time-series counters
├── trace.ndjson           # names/keys/shapes/hashes only
├── cpu/                   # perf/samply/Instruments export
├── heap/                  # private heap profile or statistics
└── checksums.sha256       # integrity for every artifact
```

Raw profiles remain private because allocation samples and process dumps can
contain user memory. CI may retain only `metadata.json`, a redacted
`report.json`, aggregate `samples.tsv`, resolved flamegraph images that contain
no data values, and checksums.

Profiler child processes run with umask `0077`. Single files and directory
bundles such as Xcode Instruments `.trace` output are checked recursively;
group/world-readable entries, symlinks and special files invalidate the run.

## 7. Release verdict

The release remains blocked when any faithful scenario:

- reaches the 85 percent guard;
- reports an OOM event, restart, signal termination, stale image, or cold-state
  mismatch;
- leaves Helixir or its backend unavailable after clean reset;
- cannot reproduce the same trace against both the mock and real database;
- lacks separate workload/database memory measurements.

Diagnostic profiles explain a failure but never convert one into a pass. Resume
the release only after the responsible component is fixed and the unprofiled
faithful differential gate passes with the original allocator and the same
representative workload.

### 7.1 Binding private runs to a public release

Raw faithful reports stay private, but a release must carry one redacted
aggregate at `helixir/doc/<tag>/memory-evidence.json`. The aggregate records
only verdicts, limits, peaks, OOM/restart counts, immutable checksums, and the
exact memory-runtime path set covered by the run. It never contains commands,
request/response bodies, private artifact paths, or credentials.

`tools/verify_release_memory_evidence.py` recomputes the path-set fingerprint
from the checked-out tag and fails closed when the evidence is missing, older
than its bounded validity window, not faithful/pass, instrumented, above the
3 GiB database or 1 GiB workload envelopes, above the 0.85 guard, or reports
an abort, OOM, restart, cold-state drift, backend-image drift, or source drift.
The primary representative workload must also have a checksum-consistent
repeat. The release workflow runs this verifier before builds begin.

Directory includes cover the complete Helixir runtime source rather than a
hand-picked module subset. Git-ignored machine artifacts such as Python
bytecode and `.DS_Store` are excluded from the fingerprint; tracked files and
non-ignored local source remain covered. This makes a faithful local run and a
clean release checkout produce the same source identity without allowing an
uncommitted runtime change to pass.

Documentation and workflow-only edits do not invalidate a faithful run because
they are outside the explicit runtime path set. Any change inside that set
changes its fingerprint and therefore requires fresh faithful evidence; do not
update the stored checksum without rerunning the real-database gate.

## 8. Ownership

- `helixdb-mock/` owns protocol emulation, bounded state, response profiles,
  and trace/metrics generation.
- `tools/memory_boundary.py` owns isolation, fail-closed sampling, profiler
  lifecycle hooks, private artifact verification, and the differential report.
- `tools/memprobe.py` remains a read-only point-in-time production diagnostic;
  it is not the release differential runner.
- `helixir/doc/test-design.md` and `helixir/doc/architecture.md` describe how
  this manifest participates in release engineering after the implementation
  is stable.
