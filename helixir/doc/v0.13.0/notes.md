# v0.13.0 — The Self-Steering Release

> _The memory now retunes itself without a reboot and saves itself from
> its own database's appetite — and the release artifacts got their
> local judge back._

## Config hot-reload — `helixir config apply` (#52)

kubectl-apply for the memory. Edit `~/.helixir/helixir.toml`, run one
command, running processes pick it up — **no Claude Desktop reboot**:

```
helixir config get [--raw]    # the resolved layered view / the raw file
helixir config set k.ey val   # dotted-path edit, comments preserved, validated
helixir config edit           # $EDITOR + validation
helixir config apply          # validate + hot-reload running MCP/gateway
```

Under the hood the MCP server and the gateway hold their client behind
an atomic swap: `SIGHUP` re-reads the config, builds and initializes a
**new** client, and swaps it in — in-flight requests finish on the old
one, a failed rebuild changes nothing. Because the whole client is
rebuilt, even database-host and LLM-provider changes hot-apply.

Honest exceptions, printed by `apply` itself: active FastThink sessions
keep their pre-reload handle; `daemon`/`watch` hold deeper snapshots and
are listed as restart-to-apply. **Transition note:** processes running a
binary older than this release EXIT on SIGHUP (no handler installed) —
restart them once on the new binary before your first `apply`.

## The watchdog now saves the database from itself (#89)

Field finding: HelixDB's in-process memory retention tracks write churn —
under a day of heavy agent traffic the container walked from 218MiB to
2.52GiB of live heap, straight toward its OOM cap. New containment:
when the **post-reclaim** (live, not cache) number crosses
`watchdog.mem_restart_pct` (92) and `allow_container_restart` is on,
Hygieia performs the supervised restart itself — volume preserved, ~10s,
journaled as `heal/mem_restarted`, alerted either way. Cache bloat can
never trigger it: the valve sheds reclaimable pages first and only
surviving pressure counts.

## NLI is back in the release artifacts (#80)

`helixir-linux-x86_64` and `helixir-windows-x86_64` ship full-featured
again: ort links against Microsoft's official ONNX Runtime release
(sha-pinned, fetched from github.com) — `cdn.pyke.io` is out of the
critical path and blackholed during the build to prove it. The tarballs
carry the runtime next to the binaries (`libonnxruntime.so.*` /
`onnxruntime.dll`) — keep them in one directory when relocating.
macOS/linux-arm64 artifacts stay lean (source-build opt-in).

## Under the hood

- The dead chunk-embedding machinery is gone (#86): memory chunks are
  raw-source storage; the retrieval unit is the extracted atom. The
  "feed it a book" idea was consciously rejected — distill books through
  the normal extraction path instead. `chunking.enable_embeddings` is
  removed from the config.
- Ten context structs replaced every 8+-argument function (#9); CI now
  denies `clippy::too_many_arguments`, so the backlog can't grow back.
  The #87 windowed-search variants merged into the main entry points.
- `helixir --version` answers (the node-update playbooks ask it).

## Upgrading

Drop-in: update the binary, restart your MCP client once (see the
transition note above). No schema changes. New knob:
`watchdog.mem_restart_pct` (92; 0 disables).
