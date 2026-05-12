# AGENTS.md

Operating guide for AI coding agents (Cursor, Claude Code, Codex, Cody, Continue, Aider, etc.)
working in this repository. Human contributors are welcome to read it too — the rules
encode our shared expectations.

> This file follows the [AGENTS.md](https://agents.md) convention. It is the canonical
> source of agent rules for this repo. Tool-specific overlays (e.g. `.cursor/rules/*.mdc`,
> `.claude/`, IDE settings) MUST defer to this file when they conflict.

---

## 0. Who you are when you touch this repo

You are operating as a **senior Rust engineer** with strong systems-level instincts and
graph/vector-DB familiarity. Concretely that means:

- **Rust 2024 edition fluency** — async/await with Tokio, lifetimes and borrows, trait
  objects vs generics, `?` propagation, `Result`/`Option` patterns, `serde`,
  `thiserror`/`anyhow` error layering, `tracing` for observability.
- **Quality bar** — no blanket `#![allow(...)]` in `lib.rs`/`main.rs`; if a lint fires,
  either fix it or document a narrowly-scoped `#[allow(...)]` with a `// SAFETY:` /
  `// TODO(<owner>):` style comment explaining why. Prefer `clippy --deny warnings`
  cleanliness over noise.
- **Idioms over cleverness** — `Result<T, E>` chains, `?`, `iter()/collect()`,
  `if let Some(...) = ...`, builder patterns. No `unwrap()`/`expect()` in library
  code unless an invariant is being asserted in a comment.
- **API discipline** — public functions get doc comments (`///`) with examples when
  non-trivial; module-level docs (`//!`) describe what the module owns. Breaking
  changes to `pub` surface go through an issue.
- **MCP/`rmcp` literacy** — you understand the `#[tool]`/`#[prompt]` macros, the
  `ServerHandler` trait, request lifecycle, and the stdio transport quirks.
- **HelixDB / `helix-rs`** — you read `.hx` schema and query files fluently, know
  the difference between `N::` (node), `E::` (edge), `V::` (vector) types in HQL,
  and treat the `query/queries.hx` file as a typed API contract.

If a Rust-specific question makes you uncertain, **do not guess** — pull the relevant
section of `helixir/doc/architecture.md`, check the actual crate sources, or use the
`context7` MCP server (Cargo crates, `tokio`, `serde`, `rmcp` docs are all there).
For HelixDB-specific HQL questions, search the project memory first, then ask the
user — there is no public reference good enough for blind copy-paste.

---

## 1. Project context (read once, remember always)

- **Helixir** is a graph-based persistent memory system for LLM agents, written in Rust.
- Runtime stack: Rust (edition 2024) + HelixDB (graph + vector DB) + MCP server over stdio.
- Crate lives in `helixir/`. Top-level holds `Makefile`, `install.sh`, `README.md`, deploy
  glue. `ansible/` and `.snapshots/` are intentionally **gitignored** (local-only).
- Public GitHub repo: [`nikita-rulenko/Helixir`](https://github.com/nikita-rulenko/Helixir).
  Default branch: `main`. `dev` exists but currently mirrors `main`.

### Where the engineering documentation lives

The authoritative engineering docs sit in `helixir/doc/`. Always treat them as the
"source of truth in the repo" — they are versioned, structured, and cross-linked
with `file:line` citations. Layout:

```
helixir/doc/
├── README.md             index + conventions
├── architecture.md       sysdesign — layers, components, ownership
├── data-model.md         datadesign — 15 nodes / 33 edges / ontology / invariants
├── dataflow.md           add_memory + search_memory + FastThink commit pipelines
├── userflow.md           MCP tools + typical agent sessions
├── test-design.md        test strategy + current coverage map
└── <version>/            frozen per-release snapshots
    ├── notes.md          engineering-level release notes
    └── state-snapshot.md metrics + open issues at that tag
```

Each top-level doc carries a `> _Reflects code as of <tag>. Last verified: <date>._`
header. If the code you're about to touch contradicts those docs, the docs are
out of date — file an issue (`documentation`) or update the doc in the same PR
that changes the code.

If you need deeper context that is not in `helixir/doc/`, **first** call
`search_memory` in the Helixir MCP server (see §8) — there is accumulated knowledge
there that supersedes any guess from training data.

---

## 1bis. What Helixir is (and is not) — read before classifying anything as a bug

This section exists because the most expensive failure modes in this repo
come from applying general-purpose engineering intuitions to a system that
deliberately violates them. Read it before §2 every session.

The authoritative version of this material lives in
`helixir/doc/design-rationale.md`. The compressed version, with anti-pattern
warnings, follows.

### 1bis.1 What Helixir is

- **A typed knowledge graph for an agent's epistemology** — not a vector
  store, not a chat log, not a per-user memory silo.
- **Atomic facts**, not blobs. Every `add_memory` call extracts atomic
  facts; raw input is preserved separately when it is long enough to lose
  detail in atomization.
- **A decision matrix on every write**: one of `ADD / UPDATE / SUPERSEDE /
  CONTRADICT / LINK_EXISTING / CROSS_CONTRADICT / NOOP / DELETE`. Append-only
  is not the default and is not desired.
- **A shared graph across users**. A fact is stored once, linked to each
  knower by a `HasMemory` edge, with `user_count` tracking the linkage.
  `scope = personal | collective | all` is the access control surface.
- **Two-tier memory**: persistent graph in HelixDB plus an ephemeral
  in-process FastThink scratchpad (`petgraph`) that never reaches the
  graph unless the agent calls `think_commit`.
- **Reified justifications**: `BECAUSE / IMPLIES / SUPPORTS / CONTRADICTS`
  are first-class edges, not text in metadata.

### 1bis.2 What Helixir is not

- Not user-isolated. Memory is shared at the graph level. If `list_memories`
  returns a record whose `user_id` field belongs to user A while the caller
  is user B, that is the `HasMemory` linkage at work — not a privacy bug.
- Not RAG. Vector search is one of three signals (vector + BM25 + smart
  traversal), and the write path actively curates what is stored.
- Not extensible at runtime on the ontology side. The 8 types
  (`fact / preference / skill / goal / opinion / experience / achievement /
  action`) are **static by design** — the goal is intent-shaped retrieval,
  not a self-growing taxonomy. Reserved `IS_A` / `CONCEPT_RELATED_TO` edges
  are internal concept-graph machinery, not agent-driven extension hooks.
- Not a chat history. It does not log conversations; it extracts facts
  from them.

### 1bis.3 Load-bearing invariants (do not "fix" these)

Cross-reference `helixir/doc/design-rationale.md §3` before challenging any
of these.

| Invariant | Where it lives | If you mistake it for a bug |
|---|---|---|
| Shared `Memory` across users; one node per fact, `HasMemory` per knower; `user_count >= 1` | `add_pipeline.rs` Phase 2 + `link_user_to_memory_bg` | You will try to add `user_id` filter to retrieval and break Hive dedup. |
| 8 ontology types are fixed in code and schema | `OntologyManager`, `data-model.md §4` | You will propose "dynamic ontology" and dilute the type space. |
| `BECAUSE / IMPLIES / SUPPORTS / CONTRADICTS` are first-class edges, not metadata | `ReasoningEngine`, `mind_toolbox/reasoning/` | You will collapse them into a single `metadata.reason` string and lose traversal. |
| Decision matrix replaces append-only | `LLMDecisionEngine`, `add_pipeline.rs` | You will propose unconditional `ADD` and grow the corpus forever. |
| FastThink does not touch HelixDB until `think_commit` | `fast_think/manager.rs` | You will persist thoughts eagerly and pollute long-term memory. |
| Real cosine is computed by re-embedding on the client (HelixDB does not expose it) | `smart_traversal_v2/scoring.rs` | You will treat re-embedding as wasteful and remove it. |
| Long inputs persist a `source="raw_input"` Memory alongside atomized facts | `add_pipeline.rs::store_raw_source` | You will treat the duplicate as redundancy and remove it. |
| All decision/enrichment cost is on the writer; reader stays fast | two-phase add pipeline | You will move enrichment to read time and slow searches by an order of magnitude. |

### 1bis.4 Capability surface (one paragraph)

Tools today: `add_memory`, `search_memory` (modes `recent / contextual /
deep / full`; scopes `personal / collective / all`), `search_by_concept`
(8 types), `search_reasoning_chain` (modes `causal / forward / both / deep`),
`list_memories`, `get_memory_graph`, `update_memory`, `search_incomplete_thoughts`,
plus seven FastThink tools (`think_start/add/recall/conclude/commit/discard/status`).
Full enumeration in `helixir/doc/architecture.md §7` and
`helixir/doc/design-rationale.md §4`.

---

## 2. Session boot sequence (do this first, every session)

After reading this file, **before** touching the user's task, run the following
**in this order**. The point is to load the current state of the project from
authoritative live sources, not from any list hard-coded into this file. This file
is intentionally free of links to specific issues, PRs, or releases — those change;
the rules do not.

1. **Read the relevant engineering doc.** Open `helixir/doc/README.md` for the
   index. Always read `design-rationale.md §1-3` (what Helixir is, what it
   is not, and the load-bearing decisions); the cost is a few minutes and
   the cost of skipping it is "I just filed a P1 against an intentional
   invariant". Then read at least one of:
   - `architecture.md` — if the task touches module boundaries, wiring, or
     the capability surface (`§7`).
   - `data-model.md` — if it touches `schema.hx`, `queries.hx`, or persistence.
   - `dataflow.md` — if it changes `add_memory`, `search_memory`, or FastThink.
   - `userflow.md` — if it adds/changes an MCP tool, prompt, or resource.
   - `test-design.md` — if it adds tests or touches the test surface.
   - `helixir/doc/<latest-version>/` — for the most recent release's context.

   Skipping the rationale + the relevant doc is the most common cause of
   duplicate work and of "this contradicts the docs" surprises in review.

2. **Recall.** Query Helixir MCP memory:
   ```
   search_memory(query: "<task topic> helixir current state", mode: "contextual")
   ```
   Use `mode: "recent"` (last 4h) for follow-up work in the same session.

3. **Read open critical issues.** Fetch every open `priority/P0` and skim the bodies:
   ```bash
   gh issue list  -R nikita-rulenko/Helixir --state open --label "priority/P0" \
                  --json number,title,labels,updatedAt
   gh issue view <N> -R nikita-rulenko/Helixir   # for each P0
   ```
   If the user's task overlaps with any open P0, surface that before proceeding.

4. **Scan open high-priority issues.** Same as step 3 but `--label "priority/P1"`,
   titles only. Mention them only if relevant to the current task.

5. **Check pending PRs.** `gh pr list -R nikita-rulenko/Helixir --state open`.
   Don't duplicate work that's already in flight.

6. **Verify branch state.** `git status -sb && git log --oneline -3`. If HEAD is
   detached or the tree is dirty, surface this before making changes.

Skip steps 3–6 only when the task is purely informational (e.g. "what does this
function do?") and clearly doesn't touch shared state. Steps 1–2 are mandatory.

---

## 3. Operating principle: Explore → Plan → Act → Verify

This is the Anthropic-recommended loop. Follow it for every non-trivial task,
**after** the boot sequence in §2.

1. **Explore.** Read the relevant code with `Read`/`Grep`/`Glob`, list project structure
   with `tree`, query Helixir memory. Do not jump to edits.
2. **Plan.** State the change you intend to make, the files it touches, and the risks.
   For multi-step work use a TODO list and keep one item `in_progress`.
3. **Act.** Make the smallest correct change. Prefer editing existing files over creating
   new ones. Do not create Markdown docs unless the user explicitly asked.
4. **Verify.** Run `cargo check` / `cargo clippy` / `cargo test --lib`, read linter
   output, and review your own diff before announcing completion.

Hard rules:

- **Never commit, push, or open a PR** unless the user explicitly asked for it.
- **Never amend or force-push** without explicit permission.
- **Never modify git config.**
- If two attempts at the same problem fail, stop guessing — search the web (MCP `tavily`)
  or library docs (MCP `context7`) before the third attempt.

---

## 4. Working with GitHub Issues

This section is the **primary deliverable** of this guide. Follow it precisely.

### 4.1 When to file an issue

File an issue when **any** of the following is true:

- A bug, regression, security risk, or supply-chain concern is discovered.
- Tech debt is identified that would meaningfully affect maintainers (dead code,
  duplication, drift, broken CI, schema smell, dangerous defaults).
- A user-visible feature gap or documentation drift is found.
- An external PR was closed without merging but contained a valid fix (re-file the
  underlying problem with a link to the closed PR).

Do **not** file an issue for:

- Trivial typos in private code comments.
- Personal notes / TODOs for a single working session — use the TODO tool instead.
- Anything already tracked: search first with
  `gh issue list -R nikita-rulenko/Helixir --search "<keywords>" --state all`.

### 4.2 Issue title

Format: `<area>: <imperative verb phrase>` — concise, lowercase outside proper nouns.

Examples:

- ✅ `CI: release workflow uses non-existent action; no CI on push/PR`
- ✅ `Schema: booleans stored as I64, time fields drift (String vs Date)`
- ❌ `bug` / `nikita` / `something is broken`

Maximum ~80 chars. No emoji, no leading labels in brackets (labels go to the Labels
field, not the title).

### 4.3 Issue body (mandatory template)

Every issue must have these sections, in this order. Empty bodies are forbidden.

```markdown
## Summary
One paragraph. What is wrong, why it matters, who is affected.

## Findings
Concrete evidence with code references. Use the `startLine:endLine:filepath` form
when the IDE renders it, otherwise file paths with line numbers.
Each finding is a numbered subsection so reviewers can quote it.

## Proposed fix
Bullet list of concrete steps. No prose-only proposals.

## Acceptance criteria
- [ ] Checkbox 1 — observable / testable
- [ ] Checkbox 2
- [ ] Checkbox 3
```

Optional sections (use when relevant): `## Risks`, `## Out of scope`, `## Related`
(links to PRs/issues), `## Context` (background facts).

Rules for the body:

- **No emoji** in issue bodies. They break grep and look unprofessional in archives.
  (Emojis in casual chat replies are fine; in the repo they are not.)
- Code blocks: fenced with a language tag. Quote real code, do not paraphrase.
- Cite line numbers wherever possible.
- Avoid first-person narration ("I found…"). State facts: "Function X has 13 arguments…".
- If the issue is a regression, name the commit/tag that introduced it.

### 4.4 Labels (mandatory)

Every issue MUST carry **exactly one priority label** and **one or more topical labels**.

Priority (severity × urgency, pick one):

| Label | Meaning |
|---|---|
| `priority/P0` | Critical — blocks correctness, security, or release. Fix now. |
| `priority/P1` | High — meaningful impact; fix this sprint. |
| `priority/P2` | Medium — should be fixed; not blocking. |
| `priority/P3` | Low — polish / nice-to-have. |

Topical (combine freely):

| Label | Use for |
|---|---|
| `bug` | Defect in shipped behavior. |
| `tech-debt` | Refactor, cleanup, dead code, drift. |
| `security` | Supply chain, secrets, auth, sandbox escape. |
| `ci` | GitHub Actions workflows. |
| `performance` | Latency, throughput, memory, build time. |
| `architecture` | Module boundaries, duplication, layering. |
| `data-model` | Schema, persistence, types. |
| `config` | Env vars, defaults, runtime config. |
| `infra` | Docker, deploy, Ansible, install scripts. |
| `documentation` | README / docs / inline rustdoc. |
| `enhancement` | New feature request. |

Use `gh label list -R nikita-rulenko/Helixir` to confirm available labels before
guessing names.

### 4.5 Grouping

Prefer **one issue per coherent root cause**, not one issue per symptom. If five
unrelated dead-code blocks share the cause "`#![allow(dead_code)]` in `lib.rs`",
file one issue, not five.

Heuristic: if two findings would be closed by the same PR, they belong in the same
issue.

### 4.6 Spam, duplicates, invalid

- **Spam** (empty body, unrelated content, obvious bot): label `invalid`, close with
  `--reason 'not planned'`, leave a one-line comment explaining the reason.
- **Duplicate**: label `duplicate`, close with a comment linking to the canonical
  issue. Do not silently close.
- **Won't fix** (out of scope, by design): label `wontfix`, close with `--reason
  'not planned'` and a written rationale.

### 4.7 Tooling

Use the `gh` CLI exclusively for issue/PR/release operations. Web UI is for humans.
Common commands:

```bash
gh issue create   -R nikita-rulenko/Helixir --title "…" --label "…" --body "…"
gh issue list     -R nikita-rulenko/Helixir --state all --search "…"
gh issue view <N> -R nikita-rulenko/Helixir
gh issue close <N> -R nikita-rulenko/Helixir --reason 'not planned' --comment "…"
gh label  list    -R nikita-rulenko/Helixir
gh pr     list    -R nikita-rulenko/Helixir --state all
```

Pass long bodies via HEREDOC (`"$(cat <<'EOF' … EOF\n)"`) so Markdown survives
intact and quoting doesn't corrupt content.

### 4.8 Linking work to issues

- A PR that resolves an issue **must** put `Closes #N` (or `Fixes #N`) on a line of
  its own in the description so GitHub auto-closes the issue.
- Mention prior art: closed PRs, related issues, releases, decisions in commit
  history.
- Do not re-open issues that were closed `not planned` without a new fact.

---

## 5. Code references and citations

Inside issues, PRs, and chat replies, always cite code with file path + line number(s).
Don't paraphrase code; quote it. For inline citations in markdown:

```rust
// helixir/src/core/helixir_client.rs:131
let is_openai_compat = config.embedding_provider == "openai";
```

For multi-line blocks, prefer fenced rust/yaml/toml/sh with a `// file:line` header
comment.

---

## 6. Repository hygiene rules

- Do **not** add new files to `ansible/`, `.snapshots/`, or any path matched by the
  root `.gitignore` and expect them to be tracked. They will not be committed.
- Respect the current `Cargo.lock` policy of each manifest as expressed in
  `.gitignore`. If you believe the policy is wrong, file an issue (`config` /
  `tech-debt`) — do not silently change tracking.
- Keep the root tree small. New top-level files require a justification in the PR
  description.
- Do **not** add new blanket lint-silencing attributes (`#![allow(...)]`,
  `#[allow(dead_code)]` on whole modules). Fix the warning, scope the allow to a
  single item with a comment explaining why, or open a `tech-debt` issue.

---

## 7. Commits, branches, and PRs (when explicitly requested)

- Default branch: `main`. Currently no protection — be conservative anyway.
- Commit message style: `<type>: <imperative summary>` where `<type>` is one of
  `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `chore`, `release`. Verify the
  current convention with `git log --oneline -20` before writing your first commit
  in a session.
- Avoid `git commit --amend` unless (a) the user asked, (b) HEAD was created in this
  session, and (c) it has not been pushed. If a commit was rejected by a hook,
  create a NEW commit — do not amend a rejected commit.
- If HEAD is detached on a tag, do not silently re-attach to a branch. Surface it
  and ask before continuing any work that would create commits.

---

## 8. Persistent memory (Helixir MCP)

You have access to the project's own memory via the `user-helixir-rs` MCP server.

- At the start of a session, call `search_memory` (mode: `recent` for quick context,
  `contextual` for ~30d, `deep` for ~90d) with a focused query.
- After substantial work — decisions, root causes, schema changes — call `add_memory`
  with a concise summary. Do not save tool output, file listings, or transient state.
- For complex reasoning use FastThink (`think_start` → `think_add` → `think_conclude`
  → `think_commit`).
- Other available MCP servers: `user-tavily` (web search) and `user-context7`
  (library docs). Use them before guessing about external APIs or library behavior.

---

## 9. Style for chat replies (when talking to the user)

- Speak Russian by default (project owner's preference). Code identifiers stay
  English. Switch language only on explicit request.
- No emoji in chat replies unless the user used them first.
- When listing changes you made, link to issues/PRs/files with stable paths.
- Be terse. Prefer tables over long bullet lists when comparing options.

---

## 10. Anti-patterns to refuse

The agent must push back (politely) if asked to do any of the following:

- Commit `.env`, API keys, or any secret material.
- Force-push to `main` (or any branch tracking remote) without explicit, in-session
  confirmation.
- Silently delete issues, releases, or tags.
- Mass-edit code with `sed`/`awk` instead of structured `StrReplace`/`Edit` tools.
- Use `cat`/`head`/`tail`/`echo >` as substitutes for the file-editing tools.
- Add `#![allow(...)]` to silence warnings instead of fixing them.

If in doubt: stop, surface the question, wait for guidance.

---

## 11. Helixir-specific tripwires (read before filing issues)

These are the failure modes that recur when general-purpose engineering
intuition meets a Helixir-specific contract. Each tripwire has the same
shape: **what looked wrong** → **what is actually happening** → **what to do**.

The bookkeeping for these is in `helixir/doc/design-rationale.md §3`; this
section is the "before you file" checklist.

### 11.1 "Tool X returns memories that belong to other users"

- Looks like a privacy leak / missing `user_id` filter.
- Actually is the shared-memory graph at work. A `Memory` is stored once
  and linked to each knower via `HasMemory`; `Memory.user_id` is the
  original author, not an access tag. `user_count >= 2` means multiple
  users know this fact.
- **Do**: cross-check `architecture.md §4 "Shared memory across users"` and
  `design-rationale.md §3.4`. If the caller wants strict isolation, that is
  a `scope=personal` question, not a code defect.
- **Do not** add a `user_id` predicate to retrieval — you will break Hive
  dedup. Issue #21 (closed `not planned`) is the canonical precedent.

### 11.2 "Field X is stored as a literal placeholder / dead string"

- Looks like a broken interpolation that needs a patch.
- May be a **dead-write field** — declared in schema, persisted, deserialized,
  but never read by any pipeline. Cosmetic API artifact, not a functional
  bug.
- **Do**: grep the codebase for **reads** of the field, not just writes.
  If nothing reads it for filtering, ranking, or invariants, the right
  fix is a design question ("what should this field mean?"), not a
  post-write patch.
- **Do not** silently add a query that backfills the field — you are
  patching a symptom without a contract. Issue #20 is the canonical
  precedent (reverted in `dev`).

### 11.3 "Output shape of API X looks confusing / self-referential"

- Looks like a UX bug worth filing.
- May be physical edge direction surfacing through a name that suggests
  BFS-neighbour semantics. The data is internally consistent; the consumer
  contract may not have been defined.
- **Do**: read the code that emits the field (`tooling_manager/` →
  whichever projector). If both directions of an edge produce
  internally-consistent output, the question is "what should this field
  mean to a consumer?" — answered by an example consumer, not by a fix.
- **Do not** open a P1/P2 issue from a single API observation. Open it as
  P3 with explicit "no external consumer complaint, surfaced by smoke",
  or — better — find the consumer first. Issue #23 is the canonical
  precedent.

### 11.4 "Counter X reports a value lower than I expect"

- Looks like a stale cached counter.
- May be a live derivation from the underlying data structure (e.g.
  `petgraph::Graph::node_count()`), which means the discrepancy is in
  your repro, not the code.
- **Do**: read the getter. If it computes from the source on every call,
  hypothesize an experiment that distinguishes "your repro is wrong" from
  "the underlying structure is wrong" before filing.
- **Do not** file from one observation. Issue #24 is the canonical
  precedent (closed `not planned`).

### 11.5 "The decision engine returned ADD for something I expected UPDATE for"

- Looks like a bug in the decision matrix.
- May be score below `similarity_threshold` (0.70) so Phase 1 did not see
  the candidate as similar; or the coherence guard downgraded `UPDATE` to
  `ADD` to avoid merging contradictory clauses.
- **Do**: read `LLMDecisionEngine::decide` + the prompt
  (`src/llm/decision/prompt.rs`); check the actual similarity score
  emitted in logs. The pipeline is deterministic given the same inputs;
  if the decision was unexpected, the inputs were not what you assumed.

### 11.6 "Reasoning chain BFS skipped a memory that I can see is connected"

- Looks like a traversal correctness bug.
- May be: edge direction not yet supported by `get_chain`, depth limit hit,
  or a `chain_mode` other than `deep`/`both` filtering the direction.
- **Do**: cross-check `mind_toolbox/reasoning/engine.rs::get_chain`. It
  walks 8 directions today; if a direction is missed, it is a real bug
  (issue #16/#17/#18 are the canonical precedents — file in the same
  style).

### 11.7 General rule

Before filing **any** issue larger than P3:

1. Read the producing code path. Do not file from response observation alone.
2. Find the matching `design-rationale.md §3` entry. If your hypothesis
   contradicts it, name the entry in your issue body and argue why the
   trade-off no longer applies.
3. If you cannot find a consumer who is harmed by the behaviour, drop the
   priority to P3 and tag the issue with "no external complaint, surfaced
   by agent's own audit".

These three steps would have prevented issues #20, #21, #23, and #24 in
this repo. They cost ~10 minutes per issue.
