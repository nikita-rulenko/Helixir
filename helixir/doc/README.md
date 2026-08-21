# Helixir — internal documentation

This folder is the engineering source-of-truth for the Helixir codebase. The
README (in the repo root) is product-facing; everything here is for contributors
who need to reason about the system, the data, the flows, and the tests.
The write-path constitution lives next door in `../memory-charter.md`; the
project vocabulary (PPR, RRF, apophenia gate, the Moirai...) is defined once
in the root-level `GLOSSARY.md` — link to it instead of re-defining terms.

## Layout

```
doc/
├── README.md             this index
├── installation.md       packages, onboarding, topology, models and clients
├── operations.md         CLI, RBAC, config, gateway, Moirai, Hygieia and admin UI
├── architecture.md       sysdesign: layers, components, ownership, capability surface
├── data-model.md         datadesign: nodes, edges, ontology, invariants
├── dataflow.md           how data moves: add_memory + search + FastThink pipelines
├── userflow.md           MCP tools and typical agent sessions
├── test-design.md        what is tested, what is not, what to add next
├── retrieval-research.md historical research record behind the default algo_opt profile
├── design-rationale.md   what Helixir is, evolution by release, and WHY
│                         the load-bearing decisions are the way they are
└── <version>/            per-version snapshot (release notes, state)
    └── notes.md
    └── state-snapshot.md
```

`codebase-audit.md`, `changes-2026-06-*.md`, `retrieval-research.md`, and
`moira.private.md` are dated engineering/design records,
not descriptions of the current branch. Their old branch names, counts and
tool totals are intentionally preserved as historical evidence. Current facts
belong in the evergreen references above or the newest version draft.

## Conventions

- **Evergreen vs. snapshot.** Files at the top level describe the system as it
  exists in `main`. Files under `<version>/` describe a specific release and
  must not be edited after that release is cut — they are historical record.
- **Version pinning.** Every top-level doc carries a header line of the form
  `> _Reflects code as of `<version>`. Last verified: `<YYYY-MM-DD>`._` Update
  both fields whenever you re-read the doc against fresh code.
- **Diagrams.** Use inline Mermaid for relationships that are materially easier
  to understand visually; GitHub renders it natively and reviewers can diff the
  source. Keep simple sequences readable as text or compact ASCII. Do not check
  in a second `.mmd` copy or a hand-exported PNG/SVG of the same diagram — those
  drift silently. A standalone image asset is acceptable only when it conveys
  information Markdown/Mermaid cannot and its update owner is documented.
- **Citations.** When referring to code, cite `<file>:<line>` (or a range).
  Example: `helixir/src/mcp/server.rs:128-168`.
- **Markdown only.** No `.d2`, `.puml`, `.mmd` checked in here. The previous
  `helixir/diagrams/` folder is deprecated; if a diagram source-format ever
  comes back, it goes in its own toolchain folder, not here.

## Reading order for newcomers

1. The root **`README.md`** — product identity, Quick Start and visual model.
2. **`design-rationale.md`** — what Helixir is, what it is not,
   and why the load-bearing decisions are the way they are. Without this
   the rest reads like generic graph-DB plumbing.
3. `architecture.md` — get the mental model of the layers and the
   capability surface (`§7`).
4. `data-model.md` — understand what is persisted and why.
5. `dataflow.md` — follow one `add_memory` and one `search_memory` end to end.
6. `userflow.md` — see how an agent actually uses the system.
7. `installation.md` / `operations.md` — deploy or operate the product.
8. `test-design.md` — learn which assertions guard which parts.
9. The latest `<version>/notes.md` for the diff from the previous release.

The newest directory may be an explicitly labelled **unreleased** draft while
a release is being assembled. Frozen snapshots begin only once the matching tag
is cut; until then the draft must describe local readiness honestly and must not
claim that package channels or images have already been published.

## Capability map

Use this table as the maintained index of shipped product surfaces. It points
to the contract that owns each capability rather than duplicating the contract
in this README.

| Capability | Primary reference |
|---|---|
| Product identity, non-goals, decision matrix, fixed ontology | [design-rationale.md](design-rationale.md) |
| Native packages, release installer, source build, three database topologies, models and MCP client registration | [installation.md](installation.md) |
| Every public CLI family, configuration, RBAC, gateway, Moirai, Hygieia, control plane and backup vault | [operations.md](operations.md) |
| MCP tools, prompts, resources, identities, result interpretation, flashbacks, outbox notices and agent lifecycle | [userflow.md](userflow.md) |
| Atomic writes, scoped dedup, hybrid retrieval, result projection and FastThink persistence | [dataflow.md](dataflow.md) |
| Rust layers, component ownership, caches, background agents and policy boundaries | [architecture.md](architecture.md) |
| Nodes, vectors, typed edges, ontology, RBAC graph and migration rules | [data-model.md](data-model.md) |
| Unit, live HelixDB, MCP, browser, installer, package and recovery evidence | [test-design.md](test-design.md) |
| Terms such as Hive, flashback, family collapse, Moirai, Hygieia and presence TTL | [../../GLOSSARY.md](../../GLOSSARY.md) |

Release directories answer only “what changed in that release”. They are not
the place to discover current behavior.

## Where to file changes

| Change | File |
|---|---|
| New module / refactor crossing layer boundaries | `architecture.md` |
| New node, edge, or schema invariant | `data-model.md` |
| New pipeline phase or order change | `dataflow.md` |
| New MCP tool, prompt, or resource | `userflow.md` |
| New test (or deliberate gap) | `test-design.md` |
| Load-bearing design decision (or a documented reversal) | `design-rationale.md` |
| Install method, topology, model or MCP registration | `installation.md` |
| CLI, RBAC, config, gateway, daemon, watchdog or admin operation | `operations.md` |
| Anything tied to one release | `<version>/notes.md` |

If a finding does not fit any of the above, prefer extending an existing file
over creating a new one. The folder is intentionally flat.
