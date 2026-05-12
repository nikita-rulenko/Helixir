# Duplication audit

> _Reflects code as of `beautify` (post-dedup). Last verified: 2026-05-12._

Single source of truth for **same-purpose code written more than once** in the
Helixir crate. Every entry has a GitHub issue and a code citation. If you
fix one of these, close the issue and strike the entry here (do not delete —
historical record).

This document is the audit summary. The detailed proposals live in the issues.

---

## 1. Findings (by severity)

| ID | Kind | What | Severity | Issue | Status |
|---|---|---|---|---|---|
| ~~D1~~ | Function | ~~`cosine_similarity` defined twice with **different semantics** (raw `[-1,1]` vs normalized `[0,1]`)~~ | P0 | [#25](https://github.com/nikita-rulenko/Helixir/issues/25) | **resolved on `beautify`**: live copy renamed to `cosine_score` in `smart_traversal_v2/scoring.rs` (semantic name reflects `[0,1]` range); dead twin in `integrator/similarity.rs` excluded from the build by disabling `pub mod integrator;` in `mind_toolbox/mod.rs`. |
| ~~D2~~ | Pipeline | ~~`smart_traversal_v2/` and `onto_search/` are two parallel search pipelines~~ | P1 | [#26](https://github.com/nikita-rulenko/Helixir/issues/26) | **resolved on `beautify`**: `onto_search/` excluded from the build via `// pub mod onto_search;` XML-block in `search/mod.rs`. Live pipeline is now exclusively `smart_traversal_v2/`. Code kept on disk. |
| ~~D3~~ | Function | ~~`safe_truncate` defined twice (canonical in `utils.rs`, private copy in `tooling_manager/helpers.rs`)~~ | P2 | [#27](https://github.com/nikita-rulenko/Helixir/issues/27) | **resolved on `beautify`**: private copy in `helpers/mod.rs` removed; all four call sites switched to `use crate::safe_truncate;`. |
| ~~D4~~ | Naming | ~~`ReasoningEngine` is **both** a `pub trait` (`integrator/reasoner.rs`) and an unrelated `pub struct` (`reasoning/engine.rs`)~~ | P3 | [#28](https://github.com/nikita-rulenko/Helixir/issues/28) | **resolved on `beautify`**: trait disappears from the namespace together with `integrator/`. Struct `ReasoningEngine` in `reasoning/engine.rs` is now the unique definition. |

All four duplications are closed by the `beautify` branch. The remaining
audit material below records the **dead infrastructure** that was excluded
from the build but kept on disk, so a future contributor does not re-discover
it.

---

## 2. Severity rubric used in this audit

| Label | Meaning here |
|---|---|
| P0 | Same name + different semantics. A future re-import flips behavior silently — no compiler warning. |
| P1 | Whole subsystems duplicated. Maintenance cost compounds; bug fixes land in one branch but not the other. |
| P2 | Same body, same semantics, two definitions. Costs ~5 min to fix; living with it costs nothing today but trains people to ignore duplicate-symbol search results. |
| P3 | Naming collision with no functional consequence. Polish. |

The maintainer asked the audit to "mark as critical". Only D1 is genuinely
critical (silent semantics divergence). D2/D3/D4 are listed at their
realistic severities to keep the priority shelf meaningful — see
`AGENTS.md §4.4`.

---

## 3. Dead infrastructure (excluded from the build, kept on disk)

These modules are declared in source but **not** part of the live
compilation unit. Each was disabled via an XML-block `// <unused
reason="...">` around its `pub mod ...;` declaration in the parent
module. Kept on disk to make a future revival cheap and to avoid
re-discovering them in the next audit.

| Module | LOC | Disabled in | `::new(` call sites outside the module |
|---|---|---|---|
| `core/services/chunking/` (`ChunkingService`, splitters, 6 events) | ~600 | not yet | 0 |
| `core/services/resolution/` (`IDResolutionService`, `BatchIDResolver`) | ~360 | not yet | 0 |
| `core/services/linking/` (`LinkBuilder`, link events) | ~280 | not yet | 0 |
| `toolkit/mind_toolbox/integrator/` (`MemoryIntegrator`, `SimilarMemoryFinder`, `EdgeCreator`, `RelationInferrer`, `trait ReasoningEngine`, `cosine_similarity`, `batch_cosine_similarity`) | ~520 | `mind_toolbox/mod.rs` (beautify) | 0 |
| `toolkit/mind_toolbox/search/onto_search/` (`OntoSearchConfig`, `OntoSearchResult`, `vector_search_phase`, `graph_expansion_phase`, `rank_results`, `classify_query_concepts`, …) | ~430 | `search/mod.rs` (beautify) | 0 |

### Notes per subsystem

- **`integrator/`** was the dead twin of `tooling_manager/add_pipeline/`.
  Excluding it removed the silent semantics divergence in
  `cosine_similarity` (D1) and the `ReasoningEngine` naming collision (D4).
- **`search/onto_search/`** was the dead twin of `search/smart_traversal_v2/`.
  Same-name phase functions (`vector_search_phase`, `graph_expansion_phase`)
  and a parallel result type `OntoSearchResult`. Excluding it closes D2.
- **`core/services/*`** is the historical event-driven implementation of
  chunking/resolution/linking. The live chunker is
  `mind_toolbox/chunking::ChunkingManager`, wired into `ToolingManager` at
  `tooling_manager/mod.rs:35`. Treatment is still deferred — the maintainer
  has not yet decided revive vs delete.

When the maintainer is ready to either resurrect or remove dead code, the
relevant precedent is `helixir/doc/design-rationale.md` §3.9 (raw-source
preservation is the closest live cousin of the event-based chunker).

---

## 4. How this file is maintained

- Add a row to §1 the moment a new duplicate is found in audit. File a
  corresponding GitHub issue (label `tech-debt` + appropriate priority).
- When an issue closes, strike (do not delete) its row in §1 and add a
  one-line note pointing at the fix commit.
- §3 is append-only until the maintainer makes the keep/delete call.
- Do not redefine the severity rubric in §2 silently. Update it in the
  same PR that introduces new categories.

---

## 5. Cross-references

- `helixir/doc/architecture.md` §7 — capability surface (what the system
  is supposed to expose).
- `helixir/doc/design-rationale.md` §3 — load-bearing decisions; if a
  proposed deduplication contradicts a §3 entry, cite the §3 entry in the
  issue body before proposing the fix.
- `AGENTS.md` §1bis, §11 — tripwires for "this looks like a bug but is
  intentional"; consult before re-classifying a duplicate as a bug.
