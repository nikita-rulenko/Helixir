# Duplication audit

> _Reflects code as of `dev` @ 554f476. Last verified: 2026-05-12._

Single source of truth for **same-purpose code written more than once** in the
Helixir crate. Every entry has a GitHub issue and a code citation. If you
fix one of these, close the issue and strike the entry here (do not delete —
historical record).

This document is the audit summary. The detailed proposals live in the issues.

---

## 1. Findings (by severity)

| ID | Kind | What | Severity | Issue |
|---|---|---|---|---|
| D1 | Function | `cosine_similarity` defined twice with **different semantics** (raw `[-1,1]` vs normalized `[0,1]`) | P0 | [#25](https://github.com/nikita-rulenko/Helixir/issues/25) |
| D2 | Pipeline | `smart_traversal_v2/` and `onto_search/` are two parallel search pipelines (same-name phase functions, parallel `SearchResult`/`OntoSearchResult`) | P1 | [#26](https://github.com/nikita-rulenko/Helixir/issues/26) |
| D3 | Function | `safe_truncate` defined twice (canonical in `utils.rs`, private copy in `tooling_manager/helpers.rs`) | P2 | [#27](https://github.com/nikita-rulenko/Helixir/issues/27) |
| D4 | Naming | `ReasoningEngine` is **both** a `pub trait` (`integrator/reasoner.rs`) and an unrelated `pub struct` (`reasoning/engine.rs`) | P3 | [#28](https://github.com/nikita-rulenko/Helixir/issues/28) |

There is a **fifth** category — wholly unused infrastructure in
`core/services/{chunking,resolution,linking}/` (~1000 LOC, zero `::new(`
call sites). Per maintainer instruction (2026-05-12) this is **not**
tracked as a duplication issue because the duplication is between
*declared but unused code* and *actually used code*. Treatment of dead
infrastructure is deferred and will be decided separately. The relevant
data points are in this file's §3 below.

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

## 3. Dead infrastructure (out of scope per maintainer)

These are not "duplicates" in the function-level sense; they are
**parallel subsystems that are declared but never instantiated**. Listed
here only so a future contributor does not re-discover them and re-file.

| Module | LOC | `::new(` call sites outside the module |
|---|---|---|
| `core/services/chunking/` (`ChunkingService`, splitters, 6 events) | ~600 | 0 |
| `core/services/resolution/` (`IDResolutionService`, `BatchIDResolver`) | ~360 | 0 |
| `core/services/linking/` (`LinkBuilder`, link events) | ~280 | 0 |

The chunker that the live pipeline actually uses is
`mind_toolbox/chunking::ChunkingManager` (343 LOC), wired into
`ToolingManager` at `tooling_manager/mod.rs:35`. The `core/services/`
chunker is event-based, designed around a `ChunkingStarted → ChunkCreated
→ ChunkLinked → ChunkChained` sequence, and was never connected to the
live flow.

When the maintainer is ready to either resurrect or remove this code, the
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
