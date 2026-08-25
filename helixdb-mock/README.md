# helixdb-mock

`helixdb-mock` is a deterministic, bounded HTTP emulator for the HelixDB v2.3.5
wire contract used by Helixir. It exists to separate Helixir process memory from
HelixDB process memory during OOM diagnosis and to make the same contract tests
repeatable in the release gate.

It is not a database replacement. It deliberately implements only the query
surface declared by `helixir/schema/queries.hx`, keeps a small coherent FIFO
state, and returns synthetic content. No production memory is copied into the
crate or written to its logs.

## Contract

- Build-time parsing validates exactly 192 unique HQL queries. A missing,
  duplicate, stale, or unclassifiable return shape fails the build.
- The data plane accepts `POST /{query_name}` with a JSON object. An optional
  `x-api-key` header is accepted and ignored.
- `GET /health` and `POST /health {}` are available for Helixir probes.
- Top-level responses are always objects. Literal HQL returns use `{"data":
  ...}`. Node, edge, and vector rows match the generated v2.3.5 handler shape.
- Missing `FIRST` or direct-ID sources return non-200
  `GRAPH_ERROR`; collection absence returns `[]`.
- Unknown routes fail closed with `404 QUERY_NOT_FOUND`. Rust call-site names
  that are absent from the canonical HQL are intentionally not invented.

The generated registry records every required source lookup. Multi-node edge
writes therefore fail unless every endpoint exists.

## Run

From the repository root:

```bash
cargo run --manifest-path helixdb-mock/Cargo.toml --locked -- \
  --listen 127.0.0.1:16969 \
  --profile recorded-v235 \
  --scenario baseline-5k
```

Configuration is also available through matching `HELIXDB_MOCK_*` environment
variables. Run `cargo run --manifest-path helixdb-mock/Cargo.toml -- --help` for
the complete bounded-response and state controls.

Latency/density profiles:

| Profile | Purpose |
|---|---|
| `fast` | 0–2 ms deterministic contract tests with tiny responses |
| `recorded-v235` | Route-class latency and redacted aggregate counts from the 5k baseline |
| `stress` | Larger but bounded payloads and elevated deterministic latency |

Fixture families are independent of latency: `bootstrap-empty`, `baseline-5k`,
`daemon-dense-category`, `rbac-multi-group`, `ingest-queue`,
`reasoning-dense`, `merge-500`, and `errors`. `merge-500` contains 500 unique,
fully scoped Memory rows for the model-free Atropos query-budget gate.
`baseline-5k` is the default coherent fixture;
`bootstrap-empty` must be selected explicitly.

## Local admin and trace

The admin plane is disabled unless `--admin-listen` is supplied, and a
non-loopback address is rejected. Its endpoints are:

- `GET /metrics` — per-route request/response bytes, latency, cardinality,
  state delta, failures, and current process RSS;
- `GET /registry` — the generated 191-query manifest and schema hash;
- `POST /control/reset` — clear bounded state and metrics.

Redacted JSONL tracing is opt-in:

```bash
helixdb-mock --trace-path /tmp/helixdb-mock-private/trace.jsonl
```

On Unix, the trace directory is forced to `0700` and the file to `0600`.
Paths inside the repository are rejected. Trace events contain hashes, field
names, response shapes/cardinalities, state counts, latency, sizes and RSS—never
raw request values, response values, API keys, or memories.

## Docker

The small crate context is paired with the canonical schema as a named BuildKit
context. This avoids sending unrelated repository artifacts to Docker:

```bash
docker build \
  --build-context helixir-schema=helixir/schema \
  -f helixdb-mock/Dockerfile \
  -t helixdb-mock:local \
  helixdb-mock
docker run --rm -p 16969:16969 helixdb-mock:local
```

## Verify and profile

```bash
cargo fmt --manifest-path helixdb-mock/Cargo.toml --all -- --check
cargo clippy --manifest-path helixdb-mock/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path helixdb-mock/Cargo.toml --locked
cargo build --manifest-path helixdb-mock/Cargo.toml --profile profiling --locked
```

The `profiling` profile is release-equivalent with line tables retained. The
faithful binary does not replace the allocator. Profile it externally, for
example with `samply record target/profiling/helixdb-mock ...` on macOS or
`heaptrack target/profiling/helixdb-mock ...` on Linux. Differential OOM
verdicts must always be reproduced with the ordinary faithful release build.
