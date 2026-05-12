# Helixir — internal documentation

This folder is the engineering source-of-truth for the Helixir codebase. The
README (in the repo root) is product-facing; everything here is for contributors
who need to reason about the system, the data, the flows, and the tests.

## Layout

```
doc/
├── README.md             this index
├── architecture.md       sysdesign: layers, components, ownership
├── data-model.md         datadesign: nodes, edges, ontology, invariants
├── dataflow.md           how data moves: add_memory pipeline + search pipeline
├── userflow.md           MCP tools and typical agent sessions
├── test-design.md        what is tested, what is not, what to add next
└── <version>/            per-version snapshot (release notes, state)
    └── notes.md
    └── state-snapshot.md
```

## Conventions

- **Evergreen vs. snapshot.** Files at the top level describe the system as it
  exists in `main`. Files under `<version>/` describe a specific release and
  must not be edited after that release is cut — they are historical record.
- **Version pinning.** Every top-level doc carries a header line of the form
  `> _Reflects code as of `<version>`. Last verified: `<YYYY-MM-DD>`._` Update
  both fields whenever you re-read the doc against fresh code.
- **Diagrams.** Use ASCII boxes inside fenced code blocks. Keep them under 100
  columns wide. Do not check in image renders — they go stale silently.
- **Citations.** When referring to code, cite `<file>:<line>` (or a range).
  Example: `helixir/src/mcp/server.rs:128-168`.
- **Markdown only.** No `.d2`, `.puml`, `.mmd` checked in here. The previous
  `helixir/diagrams/` folder is deprecated; if a diagram source-format ever
  comes back, it goes in its own toolchain folder, not here.

## Reading order for newcomers

1. `architecture.md` — get the mental model of the layers.
2. `data-model.md` — understand what is persisted and why.
3. `dataflow.md` — follow one `add_memory` and one `search_memory` end to end.
4. `userflow.md` — see how an agent actually uses the system.
5. `test-design.md` — learn which assertions guard which parts.
6. The latest `<version>/notes.md` for the diff from the previous release.

## Where to file changes

| Change | File |
|---|---|
| New module / refactor crossing layer boundaries | `architecture.md` |
| New node, edge, or schema invariant | `data-model.md` |
| New pipeline phase or order change | `dataflow.md` |
| New MCP tool, prompt, or resource | `userflow.md` |
| New test (or deliberate gap) | `test-design.md` |
| Anything tied to one release | `<version>/notes.md` |

If a finding does not fit any of the above, prefer extending an existing file
over creating a new one. The folder is intentionally flat.
