# v0.3.1-fix — engineering notes

> Tag: `v0.3.1-fix` · Released: 2026-03-27
> Compare: `git log v0.3.0..v0.3.1-fix`
> Full release notes: GitHub Releases (run `gh release view v0.3.1-fix -R nikita-rulenko/Helixir`)

This file is the **engineering-side** companion to the GitHub release notes.
It pins what changed at the code level, why it mattered, and which
contracts the fix relies on. The official release notes are the
user-facing summary; this is the source for "what should I keep in mind
when reading the v0.3.1-fix code?".

## Commits in this tag (post v0.3.0)

```
061849d  fix: reasoning chains, extraction retry, coherence validation (v0.3.1)
42e0c8d  fix: relation creation pipeline — 3 root causes for relations_created: 0
```

## What changed at the code level

### 1. Reasoning chain traversal: 3 → 8 edge directions

- **File:** `src/toolkit/mind_toolbox/reasoning/engine.rs::get_chain`
- **Before:** walked 3 of the 8 relevant edge directions; ~60% of valid
  chains were missed.
- **After:** walks both directions of IMPLIES, BECAUSE, CONTRADICTS, and
  MEMORY_RELATION (8 directions total).
- **Side effect:** new `chain_mode = "deep"` does BFS to depth 8.
- **Logging change:** silent `break` on DB error replaced with
  `warn!` + `continue`, so partial chain truncation is now observable.

### 2. Extraction robustness: retry + fallback

- **File:** `src/llm/extractor.rs::extract`
- LLM returning invalid JSON or zero memories now retries.
- After two failures, falls back to a single Memory of `memory_type="fact"`
  with the raw input text. Net result: `add_memory` cannot return an empty
  set anymore.
- `try_parse_extraction` accepts JSON wrapped in markdown fences and
  free-text-with-embedded-array shapes.

### 3. Coherence validation in dedup

- **File:** `src/toolkit/tooling_manager/add_pipeline.rs`
- New `is_coherent_memory` (line ~219) detects within a candidate memory
  contradictory clauses across distinct subjects.
- `split_incoherent_memory` (line ~263) splits at contradiction markers
  ("but", "however", "although", "...") before embedding/decision.
- `UPDATE` operations whose `merged_content` fails coherence fall back to
  `ADD` so existing memory is never corrupted by a bad merge.
- The decision prompt was updated to explicitly forbid incoherent merges
  — see `src/llm/decision/prompt.rs`.

### 4. Relation creation pipeline (the `relations_created: 0` bug)

Three independent root causes, fixed together in `42e0c8d`:

| Root cause | File | Fix |
|---|---|---|
| Cerebras expected `"json_object"`, we sent `"json"` | `src/llm/providers/cerebras.rs` (response_format) | switched to `"json_object"` |
| `enrich_memory_relations` only inferred relations on `ADD`/`SUPERSEDE`, missing `UPDATE`/`NOOP` | `add_pipeline.rs::enrich_memory_relations` (line ~562) | now runs for everything except `NOOP`/`DELETE` |
| `memories_to_store[i]` was mapped to `added_ids[j]` sequentially, breaking when some operations weren't `ADD` | `add_pipeline.rs::resolve_and_persist_extraction_relations` (line ~690) | now uses `HashMap<usize, String>` keyed by original index |

After the fix, `infer_relations` defaults to `SUPPORTS` when topics overlap
(less conservative than v0.3.1), and parses `{"relations": [...]}`-shaped
LLM responses as well as bare arrays.

### 5. Version synchronization (partial)

- `Cargo.toml`, `install.sh`, MCP `server_info.version` aligned to `0.3.1`.
- **Still wrong:** the MCP resource `config://helixir` at
  `mcp/server.rs:601` hardcodes `"version": "0.3.0"`. Tracked in issue #8;
  not fixed in this tag.

## Contracts strengthened in this release

These are the new (or newly-enforced) invariants the v0.3.1-fix code
relies on. Future refactors must keep them:

1. `add_memory` always returns `memories_added >= 1` (extraction fallback).
2. Reasoning-chain queries cover both directions of every reasoning edge.
3. `UPDATE` never persists incoherent `merged_content` — the engine
   downgrades to `ADD`.
4. Extraction → store index mapping is keyed by *original extraction
   index*, not the position in `added_ids`.

## Contracts NOT fixed by this release (carried over)

- All items in the architectural/cleanup backlog. See
  `state-snapshot.md` for the live list.
- The `config://helixir` resource still reports stale version & tool list.
- `cargo clippy` still emits 15 warnings (mostly `too_many_arguments`).
- The release CI workflow is still expected to fail on tag push due to a
  non-existent action reference — see the same snapshot file.

## Migration concerns

None. v0.3.1-fix is schema-compatible with v0.3.0; no `schema.hx` or
`queries.hx` change is required. If you upgraded HelixDB-side queries
between v0.3.0 and v0.3.1 you should already be on the v0.3.1 query set.

## Hindsight markers

Things this tag *did not* address, but became obviously problematic
while shipping it:

- The size and argument count of `add_pipeline.rs` made all three bugs
  in §4 mutually invisible. They wouldn't have lived together in a
  well-decomposed pipeline.
- The lack of CI on push/PR meant the relation pipeline was broken in
  production-shaped runs for several days before being noticed.
- The Cerebras-vs-others response format mismatch is exactly the class
  of issue that a provider trait with a typed response shape would
  prevent.

These are the seeds of the post-v0.3.1 cleanup roadmap.
