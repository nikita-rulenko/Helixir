# Helixir — internal documentation

This folder is the engineering source-of-truth for the Helixir codebase. The
README (in the repo root) is product-facing; everything here is for contributors
who need to reason about the system, the data, the flows, and the tests.
The write-path constitution lives next door in `../memory-charter.md`.

## Layout

```
doc/
├── README.md             this index
├── architecture.md       sysdesign: layers, components, ownership, capability surface
├── data-model.md         datadesign: nodes, edges, ontology, invariants
├── dataflow.md           how data moves: add_memory + search + FastThink pipelines
├── userflow.md           MCP tools and typical agent sessions
├── test-design.md        what is tested, what is not, what to add next
├── retrieval-research.md research record behind the algo_opt profile (mostly shipped)
├── design-rationale.md   what Helixir is, evolution by release, and WHY
│                         the load-bearing decisions are the way they are
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

1. **`design-rationale.md`** — start here. What Helixir is, what it is not,
   and why the load-bearing decisions are the way they are. Without this
   the rest reads like generic graph-DB plumbing.
2. `architecture.md` — get the mental model of the layers and the
   capability surface (`§7`).
3. `data-model.md` — understand what is persisted and why.
4. `dataflow.md` — follow one `add_memory` and one `search_memory` end to end.
5. `userflow.md` — see how an agent actually uses the system.
6. `test-design.md` — learn which assertions guard which parts.
7. The latest `<version>/notes.md` for the diff from the previous release.

## Where to file changes

| Change | File |
|---|---|
| New module / refactor crossing layer boundaries | `architecture.md` |
| New node, edge, or schema invariant | `data-model.md` |
| New pipeline phase or order change | `dataflow.md` |
| New MCP tool, prompt, or resource | `userflow.md` |
| New test (or deliberate gap) | `test-design.md` |
| Load-bearing design decision (or a documented reversal) | `design-rationale.md` |
| Anything tied to one release | `<version>/notes.md` |

If a finding does not fit any of the above, prefer extending an existing file
over creating a new one. The folder is intentionally flat.
