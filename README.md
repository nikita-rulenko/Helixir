<p align="center">
  <img src="helixir-logo.png" alt="Helixir" width="320"/>
</p>

<h1 align="center">Helixir</h1>

<p align="center">
  An elder brain for LLM agents: memory that never forgets,<br/>
  reasons in chains, and sees connections others can't.
</p>

<p align="center">
  <b><a href="#quick-start">⚡ Quick Start</a></b> &middot;
  <a href="#what-is-helixir">What is Helixir?</a> &middot;
  <a href="#access-control-rbac">RBAC</a> &middot;
  <a href="#contents">Contents</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/rust-1.88+-orange?logo=rust" alt="Rust 1.88+"/>
  <img src="https://img.shields.io/badge/release-v0.15.0-2ea44f" alt="Release v0.15.0"/>
  <img src="https://img.shields.io/badge/MCP-compatible-4c8bf5?logo=data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMjQiIGhlaWdodD0iMjQiPjwvc3ZnPg==" alt="MCP"/>
  <img src="https://img.shields.io/badge/license-MIT-green" alt="MIT License"/>
  <img src="https://img.shields.io/badge/HelixDB-graph%20%2B%20vector-blueviolet" alt="HelixDB"/>
</p>

---

## Contents

- [What is Helixir?](#what-is-helixir)
- [Philosophy](#philosophy)
- [Access control (RBAC)](#access-control-rbac)
- [**Quick Start**](#quick-start)
  - [One-command install](#one-command-install)
  - [Prerequisites](#prerequisites)
- [How It Works](#how-it-works)
  - [Architecture](#architecture)
  - [Read path (zero LLM calls)](#read-path-zero-llm-calls)
- [**Generative memory — the Moirai**](#generative-memory--the-moirai) — Clotho · Lachesis · Atropos
- [Ontology](#ontology)
- [Graph Schema](#graph-schema)
- [MCP Tools](#mcp-tools)
- [Glossary](GLOSSARY.md) — PPR, RRF, apophenia gate, the Moirai and the rest of the vocabulary
- [CLI](#cli) — `helixir setup` + driving the agents
- [Integration](#integration) — Cursor, Claude Desktop
- [Configuration](#configuration)
- [Development](#development)
- [Upgrading](UPGRADING.md) — version-by-version migration notes (v0.4 → v0.14)

---

## What is Helixir?

**Helixir doesn't store data — it grows minds.** The product is not this
software: it is the brain you grow with it. A graph that has been fed a
domain for a year becomes an asset in its own right — yours to keep, to
carry between models, to license, to seat as a resident advisor inside any
agent. Models come and go; a grown memory compounds.

Ordinary AI memory is similar-text retrieval. Helixir keeps the *why*:
causal chains, provenance on every fact, automatic consensus (a duplicate
from a second agent becomes confirmation, not a copy) and automatic
disagreement (a contradiction surfaces and demands resolution instead of a
silent overwrite). Knowledge here doesn't rot — it self-cleans and
appreciates.

Helixir gives AI agents **memory that persists between sessions** — and more than that: memory that *reasons*. When an agent starts a new conversation, it recalls past decisions, preferences, goals and the **chains of reasoning behind them**, not a flat log of similar text.

Every input is LLM-extracted into atomic facts, classified by ontology (8 types), linked to entities and to other facts by typed edges — causal (`BECAUSE`, `IMPLIES`, `CONTRADICTS`, `SUPPORTS`) and associative (`RELATES_TO`, `PART_OF`, `IS_A`) — and stored in one graph+vector engine. Retrieval is a hybrid of dense vectors, BM25 keyword search and graph traversal ranked by Personalized PageRank — with **zero LLM calls on the read path**, so it is exactly as fast on a local ollama model as on a cloud API.

Built on [HelixDB](https://github.com/HelixDB/helix-db) (graph + vector database) with native [MCP](https://modelcontextprotocol.io/) support for Codex, Cursor, Claude Desktop, Claude Code and any MCP-compatible client. Since v0.14.0, graph-backed RBAC is permanent: HelixDB itself owns principals, groups, roles, memory visibility and dedup federations, while `default` preserves the former full-trust workspace and `onboarding` admits new agents safely.

| Plain RAG memory | Helixir |
|:-----------------|:--------|
| Returns similar text chunks | Returns facts **with provenance**: what matched directly, what was pulled through which edge, and why |
| Append-only — grows forever | Curated writes: ADD / UPDATE / SUPERSEDE / NOOP decided per fact |
| No reasoning trail | Causal chains: *A because B*, *A implies C* — and `connect_memories(A, B)` finds the path between any two concepts |
| LLM in the retrieval loop | Read path is LLM-free: ~15–30 ms warm searches, fully local |
| Single-user silo | Shared graph: one fact, many knowers, consensus ranking, conflict detection |
| Silent overwrites | Memory charter: conflicting writes escalate to the agent as questions |

And recall is only the floor. Helixir now takes the next step — from *retrieving* chains to **generating** them: three background agents (the Moirai) weave a category layer over the graph and surface non-obvious cross-domain connections as **hypotheses with provenance**. See [Generative memory](#generative-memory--the-moirai).

## Philosophy

Three principles drive every design decision; the long version lives in [`helixir/doc/design-rationale.md`](helixir/doc/design-rationale.md).

**An elder brain forgets nothing.** There is deliberately **no delete tool**. Outdated facts are superseded — the old version stays in history (`HAS_HISTORY` edges, `valid_until`), reachable forever. Why? Because the value of memory is not in single facts but in long chains between them: *Rajasthan weather → guar harvest → guar gum price → fracking costs → shale stocks*. A memory that prunes "irrelevant" facts destroys the middle of chains it cannot yet see. Time affects **attention** (what surfaces first), never **reachability** (what can be found through connections).

**The writer pays, the reader flies.** All expensive work — extraction, dedup decisions, relation inference — happens at write time. Reading is pure math over precomputed structure: no LLM, no re-embedding when warm. This is what makes a fully local setup (ollama + HelixDB) practical.

**The memory does not gaslight its owner.** Writes that conflict with what is already known — a reversed preference, a contradiction, anything destructive — are not resolved silently. They come back in `add_memory.needs_clarification` as ready-to-ask questions, governed by a human-editable [memory charter](helixir/memory-charter.md): a constitution of rules the engine may never override.

**And the charter learns.** Every `resolve_contradiction` verdict becomes a precedent; after several identical verdicts the memory *proposes a standing rule* back to the agent (`rule_proposal`), ready to adopt with one `add_memory` call. Adopted rules render in the `memory://rules` resource beside the constitution — which itself never self-learns — and silence future questions of that shape. Corrections also win: a superseded fact ranks below its successor and returns flagged `superseded: true` with `superseded_by` naming the current version — history, honestly labelled, never hidden.

---

## Access control (RBAC)

Since v0.14.0, RBAC is **permanently enabled** and stored in HelixDB alongside
the memories it protects. Principals, groups, role assignments, assignment
history, per-memory visibility edges, dedup federations, and bootstrap state all
come from the graph. The CLI and MCP server enforce that same graph-backed
policy; there is no second ACL file to drift out of sync. The reserved `moirai`
workspace is the explicit global-admin-only layer for generated hypotheses.

Every memory visible to a non-admin belongs to a concrete visibility group.
Reads are allowed through materialized `MEMORY_IN_RBAC_GROUP` edges, not by
trusting `Memory.user_id` as an ACL. `actor_id` is the principal performing an
operation; `user_id` remains the memory owner and provenance. Authorization is
deny-by-default and fails closed when the actor, group, or deployed RBAC schema
cannot be resolved.

### Roles

| Role | Scope | Read | Write | Manage RBAC |
|:-----|:------|:-----|:------|:------------|
| `admin` | Global | Every memory and group | For any owner, in any existing group; unscoped writes stay admin-only | Yes |
| `groupadmin` | One or more assigned groups | All memories in those groups | Own memories and memories owned by group members | Membership and roles in those non-reserved groups |
| `moderator` | Assigned groups | All memories in those groups | Own memories and memories owned by group members | No |
| `worker` | Assigned groups | All memories in those groups | Only memories authored under their own `user_id` | No |
| `viewer` | Assigned groups | All memories in those groups | No | No |

A global `admin` manages the full graph, user registry, group lifecycle, reserved
workspaces, global roles, and dedup federations. A `groupadmin` is the operational
team-lead role: the same principal may administer several assigned groups and can
add, remove, or change members there. The old read-only `teamlead` grant is retired;
existing assignments remain readable until an administrator explicitly converts
them with `helixir rbac migrate-teamleads --yes`. The last global administrator
cannot be revoked. Assignments are deactivated rather than erased, so audit history
survives removal.

### Reserved workspaces and onboarding

The one-way, resumable bootstrap creates three protected groups:

- `default` receives pre-RBAC memories and previously trusted principals.
  Those principals become equal `groupadmin` peers there, preserving the old
  full-trust collaboration model without granting global administrative power.
- `onboarding` admits newly discovered users and agents as `worker`s. Their
  membership makes them visible in the graph-backed registry so an administrator
  can inspect them and assign normal working groups.
- `moirai` is a membership-free, global-admin-only system workspace. Clotho,
  Lachesis, and Atropos may analyze memories from every group, but their generated
  hypotheses and provenance are persisted here and never enter a team's ordinary
  search or dedup domain.

A normal admission flow is:

```bash
export HELIXIR_RBAC_ACTOR=root

helixir rbac user list --json
helixir rbac group create --id development --name "Development"
helixir rbac group add-user --group development --user alice --role worker
helixir rbac group remove-user --group onboarding --user alice
helixir rbac user show --user alice --json
```

Agents must pass their stable `actor_id` and the concrete access `group_id` on
`add_memory` and `think_commit`. Omitting `group_id` is accepted only when
Helixir can infer exactly one writable reserved workspace; normal working-group
writes should always name the group explicitly.

### Group-isolated and federated deduplication

Groups deduplicate independently by default, preventing one team's knowledge
from revealing or mutating another team's memories. An administrator can
deliberately place several groups in a dedup federation when they should share
both deduplication and visibility:

```bash
helixir rbac dedup create --id engineering --name "Engineering federation"
helixir rbac dedup attach --group development --dedup-group engineering
helixir rbac dedup attach --group platform --dedup-group engineering
```

Joining a federation grants the group access to its existing memory history.
Detaching is prospective: historical visibility remains, while future writes
return to the detached group's own dedup domain. The protected `default`,
`onboarding`, and `moirai` workspaces cannot join a federation.

> **Trust boundary.** RBAC separates cooperative principals inside Helixir; it
> is not identity authentication by itself. Stdio clients run locally. The
> network gateway assumes a trusted network unless bearer authentication is
> configured, and any client allowed to submit arbitrary requests can claim an
> `actor_id`. Do not expose an unauthenticated gateway to an untrusted network.

See [`helixir rbac --help`](#cli) for the management surface and
[`helixir/doc/data-model.md`](helixir/doc/data-model.md) for the persisted graph
model.

---

## Quick Start

### One-command install

```bash
curl -fsSL https://raw.githubusercontent.com/nikita-rulenko/Helixir/main/install.sh | bash
```

The script detects the host and downloads the matching release asset into a
versioned `~/.helixir/versions/<version>` directory, switches the atomic
`~/.helixir/current` pointer, then launches the guided `helixir onboard` flow.
After onboarding it starts the admin-only web control plane at
`http://127.0.0.1:6971`. The SPA and Axum backend live together in a hardened
container; the native install tree contains no Node.js or frontend assets. A
token-authenticated typed supervisor performs the small set of approved host
operations and both processes recover automatically after login/reboot on
macOS and Linux. Use `install.sh --no-web` for a fully headless CLI install.
The recommended flow installs and starts Ollama on macOS or Linux and provisions
`nomic-embed-text`. A user may instead explicitly configure an OpenAI-compatible
remote embedding provider, model, endpoint, and key. The flow also recommends an
optional local fallback LLM from detected RAM (compact `llama3.2:3b`, balanced
`qwen2.5:7b`, or `gpt-oss:20b`). Model pulls retry safely and are verified through
Ollama's local API before onboarding succeeds. The required NLI safety model is
always installed and verified. The same plan provisions a persistent
HelixDB volume with a pre-schema backup, central `helixir.toml`, and automatic
Claude Code/Codex/Cursor registration.

Onboarding distinguishes three backend contracts instead of guessing: a
Helixir-managed local HelixDB, an existing separately managed local HelixDB,
or an explicit remote endpoint. It then creates permanent graph-backed RBAC,
placing legacy shared knowledge in reserved `default` and new principals in
reserved `onboarding` before normal group assignment.

Use `--dry-run` to inspect the plan. Automation can select exactly the same
choices without prompts:

```bash
# Fully local defaults chosen for this machine
helixir onboard --non-interactive

# Explicit local fallback LLM
helixir onboard --non-interactive --local-llm-model qwen2.5:7b

# Keep the configured remote primary LLM; use local Nomic embeddings
helixir onboard --non-interactive --no-local-llm

# Explicit remote embeddings; provide the key through the protected config or env
HELIX_EMBEDDING_API_KEY=... helixir onboard --non-interactive \
  --remote-embeddings --embedding-provider openai \
  --embedding-model text-embedding-3-small \
  --embedding-url https://api.openai.com/v1
```

NLI and a verified embedding path are mandatory. Ollama plus
`nomic-embed-text` is the default; remote embeddings must be selected and fully
specified explicitly. `helixir doctor` sends a real embedding probe. If the
remote path is missing or broken, it reports the failure, installs/starts Ollama,
pulls Nomic, atomically switches the central config, and verifies the repair.
`--no-local-llm` skips only the optional fallback LLM.

Or install manually:

```bash
git clone https://github.com/nikita-rulenko/Helixir.git
cd helixir

make build          # Build release binaries for this host
make install        # Versioned install + guided onboarding
make onboard        # Re-run onboarding
make doctor         # Readiness report + automatic embedding recovery
```

The ongoing admin dashboard exposes live memory/node/agent counters, RBAC user,
group, role and dedup-federation administration, an interactive category-first
memory graph, the Moirai evidence journal, Hygieia resource telemetry, and the
same previewable/resumable installation plan as the CLI. It is deliberately
unavailable to every non-global role. Lifecycle commands are:

```bash
helixir control-plane status
helixir control-plane install
helixir control-plane uninstall
```

### Prerequisites

- **Rust 1.88+** — [rustup.rs](https://rustup.rs) (the local NLI judge is a
  required component of every build)
- **Docker** — for HelixDB ([install](https://docs.docker.com/get-docker/))
- **HelixDB CLI v2.3.5 — the version matters.** Helixir targets the v2
  (LMDB) generation of HelixDB. CLI **v3.x is NOT compatible**: it runs a
  different engine (hyperscale over object storage), has no `helix check` /
  `helix build`, and its `helix start` never compiles this repo's `.hx`
  schema — the gateway comes up with `query_count: 0` and every Helixir call
  fails. Both `curl install.helix-db.com | bash` and `cargo install
  helix-cli` install latest (v3.x) — instead, install the pinned binary from
  the GitHub release:

  ```bash
  # substitute your platform: helix-aarch64-apple-darwin, helix-x86_64-apple-darwin,
  # helix-aarch64-unknown-linux-gnu, helix-x86_64-unknown-linux-gnu (WSL2),
  # helix-x86_64-pc-windows-msvc.exe
  curl -L -o ~/.local/bin/helix \
    https://github.com/HelixDB/helix-db/releases/download/v2.3.5/helix-x86_64-unknown-linux-gnu
  chmod +x ~/.local/bin/helix
  helix --version    # must print: Helix CLI 2.3.5
  ```

  Preserved mirror (same binaries + source tag + `v2-lts` branch, in case
  upstream ever drops v2):
  <https://github.com/nikita-rulenko/helix-db/releases/tag/v2.3.5>

  There is no public HelixDB server image: the CLI builds it locally,
  compiling this repo's schema into it (`install.sh` / `make setup` do
  this for you). If you already ran a v3 CLI here, delete its instance
  and containers first (`docker rm -f` anything from
  `ghcr.io/helixdb/enterprise-dev`), then redo `make setup` with 2.3.5.

> ⚠️ **Storage-mode trap (data loss).** Newer HelixDB builds default to
> **in-memory** storage — stopping the instance ERASES everything unless it
> runs with disk persistence (`helix start dev --disk` for CLI-managed
> instances; a mounted `HELIX_DATA_DIR` for containers, as our compose and
> install script configure). After any HelixDB upgrade or fresh install,
> verify persistence: write a memory, restart the instance, confirm it
> survived. Hygieia's `storage_not_persistent` detector also alarms when a
> serving database has no LMDB files in its data dir.
- **API key** — at least one LLM provider:
  - [Cerebras](https://cloud.cerebras.ai) (free tier, ~3000 tok/s)
  - [DeepSeek](https://platform.deepseek.com) (cheap, ~$0.14/$0.28 per 1M tok)
  - [Ollama](https://ollama.com) (local, no key needed — auto-fallback when a remote provider is down)

---

## How It Works

```
           Input: "I deployed the server to AWS and prefer using Terraform"
                                      |
                                LLM Extraction
                                      |
                      +---------------+---------------+
                      |                               |
              Memory: "I deployed         Memory: "I prefer
              the server to AWS"          using Terraform"
              type: action                type: preference
                      |                               |
                +-----+-----+                   +-----+-----+
                |           |                   |           |
            Entity:     Entity:            Entity:      Concept:
            "AWS"       "server"           "Terraform"  Preference
                      |
                Phase 1: Personal search (dedup check)
                Phase 2: Cross-user search (shared facts)
                      |
                Decision: ADD / UPDATE / SUPERSEDE / NOOP
                      |
                Memory charter check ── conflicts? ──> needs_clarification
                      |                                (agent asks the human)
                Store in HelixDB (graph + vector)
```

### Architecture

```
MCP Server (stdio)                        IDE (Cursor / Claude Desktop)
       |                                           |
  HelixirClient                               MCP Protocol
       |
  ToolingManager ──── FastThinkManager
       |                    |
  +----+----+----+     petgraph (in-memory)
  |    |    |    |          |
Extract Decision Entity  commit to DB
  |    Engine  Manager       |
Search    |    Ontology      |
Engine  Reasoning Manager    |
  |    Engine    |           |
  +----+----+----+-----------+
       |
  HelixDB Client (HTTP)
       |
  HelixDB (graph + vector database)
```

### Read path (zero LLM calls)

> **Curated output.** Results are compacted before they reach the agent:
> capped at an honest top-K, deduplicated, and a raw source never coexists
> with its own extracted atoms in one window — the family collapses into its
> best-ranked member, with the folded ids kept reachable under
> `metadata.collapsed`. Compaction of redundancy, never of content: the goal
> is spending the agent's context window on distinct facts, not repeats.

```
Query ──> embedding (cached) ──┬──> dense ANN (HelixDB HNSW)   ──┐
                               └──> BM25 keyword (SearchBM25)  ──┤
                                                                 ├──> RRF fusion
                                                                 v
                              graph expansion: one batched HQL call per depth level
                              (8 edge families, parent provenance kept)
                                                                 v
                              Personalized PageRank over the typed ego-network
                              final rank = 0.3·cosine + 0.5·PPR + 0.2·freshness
                                                                 v
                    results with provenance: origin=seed|graph, edge, parent, ppr
```

Warm search: p50 ≈ 15–30 ms. Reasoning chains and `connect_memories` run on the same machinery — the read path works identically with no LLM configured at all.

> **Time windows & flashbacks.** `search_memory` takes an explicit event-time window (`time_from` / `time_to`, RFC3339 or `YYYY-MM-DD`). The window hard-filters the *seeds* — the direct answers — but graph expansion stays exempt: a memory from outside the window that is linked to an in-window result returns anyway, flagged `flashback: true` with its `event_date`, capped by a separate small allowance (`retrieval.flashback_max`, default 3) so associations never crowd the period's own rows. Like human memory: thinking about last week can surface last year — but you know it's old.

---

## Generative memory — the Moirai

The chain *Rajasthan weather → guar harvest → guar gum → fracking cost → shale stocks* is never a single stored edge — it runs through layers of abstraction. Helixir's next step is to **generate** those connections itself: three background agents, named for the Fates, spin a second axis over the flat graph and surface non-obvious cross-domain links — always as **hypotheses with provenance**, never asserted truth (the charter, extended from stored facts to generated connections).

- **Clotho — the Spinner.** Tags memories from a controlled, self-growing category vocabulary (embedding-match; on a miss it mints a fitting category via the LLM). Shared tags weave distant memories into subsets — a category layer that accretes over the graph from the corpus itself.
- **Lachesis — the Measurer.** Routes chains *within* the subsets and gates them against apophenia: a coherence gate (geometric-mean edge weight) plus **PMI subset overlap** — a thick, everything-touching category gates itself out by arithmetic. It drills every link down to the anchor memories that witness it. Her second duty is **retroactive causal stitching**: a bounded pass proposes entity-overlapping pairs of *old* memories, an LLM judge conservatively confirms explicit causation, and survivors become admin-only hypothesis memories with provenance — never asserted `BECAUSE` edges in a team's base graph.
- **Atropos — the Cutter.** Curates the survivors into ranked, deduplicated **insights** carrying provenance and a lifecycle (`proposed → verified → refuted`).

The three run as one orchestrated pass — on demand or on a schedule via the [daemon](#cli), with a per-Moira cadence (tag every pass, route insights every Nth). Only a global `admin` may invoke them. They read across all groups to find organization-wide patterns, while every surviving insight is persisted under `user_id=helixir` in reserved `moirai`, linked to witnesses by the non-traversable `MOIRAI_DERIVED_FROM` edge. Ordinary roles cannot read the hypothesis or use its category/provenance layer as a graph bridge. Drive and watch it all with the [`helixir` CLI](#cli).

> **Status.** The pipeline is built and validated end-to-end — the guar chain reconstructs as a single insight on clean data, and a live multi-agent corpus produced 5-hop cross-domain chains (weather → agriculture → petrochemicals → battery tech). Insight quality tracks tag/corpus hygiene; the provenance is what lets you tell signal from noise.

---

## Ontology

Every memory is classified into one of **8 concept types**. The LLM extractor assigns the type during ingestion; `search_by_concept` retrieves memories by type.

| Type | What it captures | Example |
|:-----|:-----------------|:--------|
| **fact** | Objective knowledge, statements about the world | "Rust compiles to native code" |
| **preference** | Likes, dislikes, tastes, favorites | "I prefer dark mode in all editors" |
| **skill** | Abilities, competencies, expertise | "I can write fluent Python" |
| **goal** | Plans, aspirations, objectives | "I want to learn Japanese this year" |
| **opinion** | Subjective beliefs, judgments, viewpoints | "I think remote work is more productive" |
| **experience** | Past events, situations lived through | "I lived in Berlin for 3 years" |
| **achievement** | Accomplished milestones, completed goals | "I built a working compiler from scratch" |
| **action** | Specific tasks performed, operations executed | "I deployed the CI/CD pipeline yesterday" |

### Ontology hierarchy

The concept types are organized into a tree stored in HelixDB:

```
Thing
  ├── Attribute
  │     ├── Fact
  │     ├── Preference
  │     ├── Skill
  │     ├── Goal
  │     ├── Opinion
  │     └── Trait
  ├── Event
  │     ├── Action
  │     ├── Experience
  │     └── Achievement
  ├── Entity
  │     ├── Person
  │     ├── Organization
  │     ├── Location
  │     ├── Object
  │     └── Technology
  ├── Relation
  └── State
```

The hierarchy enables traversal: searching for "Attribute" returns all facts, preferences, skills, goals, and opinions. Entity types (Person, Organization, etc.) are used for extracted named entities.

---

## Graph Schema

Helixir stores everything as a typed graph: **22 node types** (+ 5 vector-index types) connected by **30 edge types** — including the **category subgraph** the Moirai weave over it (`Category` / `CategoryEmbedding` nodes; `TAGGED_AS`, `SUBCATEGORY_OF`, `ALIAS_OF` edges) and the admin-only `MOIRAI_DERIVED_FROM` provenance edge.

### Node types

| Node | Purpose | Key fields |
|:-----|:--------|:-----------|
| **Memory** | Core unit — one atomic fact | content, memory_type, certainty, importance, user_id |
| **User** | Owner of memories | user_id, name |
| **Entity** | Named thing extracted from text | name, entity_type, aliases |
| **Concept** | Ontology node (Fact, Skill, Goal...) | name, level, parent_id |
| **Context** | Situational scope (work, personal...) | name, context_type |
| **Session** | Conversation session | session_id, status |
| **Agent** | AI agent that created a memory | agent_id, role, capabilities |
| **HistoryEvent** | Audit log entry for a memory | action, old_value, new_value, timestamp |
| **MemoryChunk** | Fragment of a long memory | content, position, token_count |
| **Reasoning** | Reasoning node | reasoning_type, confidence |
| **Constraint** | Rule applied in a context | rule, constraint_type, priority |
| **MemoryEmbedding** | Vector embedding (search index) | content, created_at |
| **EntityEmbedding** | Vector embedding for entity search | name |
| **DocPage / DocChunk / CodeExample / ErrorCode** | Documentation pipeline (reserved) | — |

### Memory ↔ memory relations (the edge arsenal)

All seven typed relations between memories persist as ONE physical edge —
`MEMORY_RELATION` — whose `relation_type` property names the type, so new
relation types need no schema change. Four are **causal/logical** (these form
reasoning chains and are what `search_reasoning_chain` walks); three are
**associative/structural** (relatedness without a causal claim; they surface
in `get_memory_graph`):

| relation_type | Kind | What it means |
|:--------------|:-----|:--------------|
| **IMPLIES** | causal | A logically leads to B |
| **BECAUSE** | causal | A is the reason for B |
| **CONTRADICTS** | causal | A conflicts with B |
| **SUPPORTS** | causal | A provides evidence for B |
| **RELATES_TO** | associative | Same topic / relatedness, no causal claim |
| **PART_OF** | associative | A is a part/component of B |
| **IS_A** | associative | A is a kind/instance of B |

Two dedicated memory→memory edges are written by the **decision engine**
(not the reasoning arsenal): `SUPERSEDES` (a new fact replaces an outdated
one — with reason and timestamp) and `CONTRADICTS` (a tracked, resolvable
conflict — with `resolved` / `resolution_strategy` for the reconcile pass).

### Edge types (active)

Every type below is verified against the code: it has a writer query AND a
Rust caller. (An edge type earns its place by three tests: a read-path
algorithm walks it to answer a distinct question class; it has a reliable
producer; and without it the reader would need an LLM call. Types that
failed those tests were removed in v0.9.x — see UPGRADING.)

| Edge | From → To | What it means |
|:-----|:----------|:--------------|
| **HAS_MEMORY** | User → Memory | User owns this memory (consensus `user_count` derives from these) |
| **INSTANCE_OF** | Memory → Concept | Memory is of this ontology type |
| **TAGGED_AS** | Memory → Category | Clotho's category tag (the Moirai substrate) |
| **MENTIONS** | Memory → Entity | Memory mentions this entity |
| **EXTRACTED_ENTITY** | Memory → Entity | Entity was LLM-extracted from this memory |
| **RELATES_TO** | Entity → Entity | Two entities are related (typed: works_at, uses, etc.) |
| **PART_OF** | Entity → Entity | Hierarchical entity relations |
| **VALID_IN** | Memory → Context | Memory applies in this context (work, personal...) |
| **CREATED_IN** | Memory → Session | Which session created this memory |
| **AGENT_CREATED** | Agent → Memory | Authorship provenance: this agent wrote it |
| **HAS_HISTORY** | Memory → HistoryEvent | Audit trail: who changed what and when |
| **HAS_CHUNK** | Memory → MemoryChunk | Memory split into chunks (long texts) |
| **HAS_EMBEDDING** | Memory → MemoryEmbedding | Memory's vector index for semantic search |
| **HAS_SUBTYPE** | Concept → Concept | Ontology hierarchy (Attribute → Skill) |
| **IS_A** | Concept → Concept | Dynamic ontology extension |
| **CONCEPT_RELATED_TO** | Concept → Concept | Cross-concept links |
| **ALIAS_OF** | Category → Category | Vocabulary convergence: near-synonym categories point at their canonical (Clotho wires these; mint-time convergence prevents new synonyms) |
| **MOIRAI_DERIVED_FROM** | Moirai Memory → Memory | Admin-only provenance; deliberately absent from ordinary reasoning traversal |

### Edge types (in development)

Declared in the schema with a named producer, not yet wired end-to-end:

| Edge | From → To | Planned producer |
|:-----|:----------|:-----------------|
| ENTITY_HAS_EMBEDDING | Entity → EntityEmbedding | Entity-resolution v2: persisted vectors for cross-session entity dedup (fragmented entities break graph hubs) |
| CHUNK_TO_EMBEDDING | DocChunk → ChunkEmbedding | Reserved doc pipeline. Memory-chunk vectors were rejected (#86): chunks are raw-source storage; the retrieval unit is the extracted atom |

Everything else that used to sit in a "reserved" list — duplicate twins and
an unbuilt documentation-ingestion subsystem — was removed from the schema
in v0.9.x rather than left as fiction: a type without a producer misleads
more than it reserves.

---

## MCP Tools

### Memory

| Tool | What it does |
|:-----|:-------------|
| `add_memory` | Extract atomic facts, deduplicate, store with entities and relations. Confirm-or-promise ack: `ok:true` with `memory_ids` (new), `updated` (changed), or `deduped` (already known), or `{ok:true, status:"accepted", pending_id}` under the ingest buffer. Charter conflicts come back in `needs_clarification`. Pass stable `actor_id`, the concrete working `group_id`, and optional `agent_id`; the write auto-heartbeats swarm presence |
| `get_add_status` | Poll a buffered `add_memory` by its `pending_id` (`pending`/`processing`/`done`/`failed`) |
| `search_memory` | Hybrid search (vector + BM25 + graph, PPR-ranked) with temporal `mode` (`recent`/`contextual`/`deep`/`full`) and `scope` (`personal`/`collective`/`all`). Every result carries provenance (`origin`, `edge`, `parent`, `ppr`) |
| `connect_memories` | **"How is A related to B?"** — bidirectional path discovery between two concepts; each anchor is a free-text query **or** an exact `memory_id` |
| `search_by_concept` | Filter by ontology type: skill, preference, goal, fact, opinion, experience, achievement, action |
| `search_reasoning_chain` | Traverse causal/logical connections: IMPLIES, BECAUSE, CONTRADICTS, SUPPORTS — LLM-free |
| `get_memory_graph` | Return memory as a graph of nodes and typed edges — causal (IMPLIES/BECAUSE/SUPPORTS/CONTRADICTS) plus associative (RELATES_TO/PART_OF/IS_A) |
| `list_memories` | Bulk dump for a user (newest first, no ranking) — for counting/auditing |
| `list_users` | Roster of identities (`user_id`s) for orientation — gated by the collective tier, privacy-safe (no emails/content); use it to find your own or a teammate's id |
| `swarm_status` | **Rendezvous through the DB itself**: the live agent roster (role, host, status, last-seen) — who else is working this memory right now. Collective-gated; presence comes from `add_memory` heartbeats, no side channel |
| `resolve_contradiction` | Answer a `contradiction_review` notice: `confirm` (my memory stands), `retract` (the disputing memory supersedes mine — history preserved) or `preference` (both coexist). Non-destructive in every branch |
| `agent_farewell` | Mark a one-shot agent as done in the swarm roster without changing authorship provenance |
| `update_memory` | Modify existing memory content |
| `search_incomplete_thoughts` | Find historical incomplete FastThink memories created before permanent RBAC |

### FastThink (working memory)

Isolated scratchpad for complex reasoning. Nothing pollutes long-term memory until you explicitly commit.

| Tool | What it does |
|:-----|:-------------|
| `think_start` | Open a new thinking session |
| `think_add` | Add a reasoning step (types: reasoning, hypothesis, observation, question) |
| `think_recall` | Pull facts from long-term memory into the session (read-only) |
| `think_conclude` | Mark a conclusion |
| `think_commit` | Save the conclusion to long-term memory |
| `think_discard` | Discard the session without saving |
| `think_status` | Check session state: thought count, depth, elapsed time |

**Flow:** `think_start` &#8594; `think_add` (repeat) &#8594; `think_recall` (optional) &#8594; `think_conclude` &#8594; `think_commit`

If a session times out, permanent RBAC keeps the scratchpad isolated and
fails closed because no owner/group was supplied for an automatic write.
Discard and restart it explicitly. Historical `[INCOMPLETE]` memories remain
recoverable via `search_incomplete_thoughts`.

---

## CLI

Beyond the MCP server, the `helixir` binary drives and monitors the generative agents:

```bash
helixir setup                          # interactive: configure + wire the MCP server into
                                       #   Claude Code / Claude Desktop / Cursor / Gemini CLI
helixir mode                           # show the privilege tier (solo | collective | insights)
helixir onboard                        # backend, models, RBAC, MCP, skills, doctor
helixir rbac bootstrap --operator root --principal codex --principal claude
helixir rbac status --json              # inspect the HelixDB-backed RBAC graph
helixir rbac migrate-teamleads --yes    # explicitly convert legacy read-only grants
helixir rbac group create --id alpha --name "Alpha team"
helixir rbac group add-user --group onboarding --user alice --role worker
helixir rbac group add-user --group alpha --user alice --role worker
helixir rbac grant --user root --role admin
helixir rbac check --user alice --action read --owner bob
helixir model download | status        # fetch / inspect the local NLI judge (ONNX weights)
helixir gateway start | status | stop  # serve MCP over the network (streamable-HTTP, #42)
helixir categories                     # the category dictionary + member counts (coverage)
helixir clotho grow --user <id>        # tag a user's memories, growing the dictionary on misses
helixir lachesis route --seed <cat>    # route a cross-domain subset thread (with witnesses)
helixir atropos                        # curate threads into ranked, journaled insights
helixir pipeline --user <id>           # one orchestrated pass: Clotho → Lachesis → Atropos
helixir daemon start --user <id> --interval 600   # run passes in the background
helixir daemon status | stop           # inspect / stop the background daemon
#   per-Moira cadence: --clotho-every 1 --insight-every 3 --merge-every 5 --reconcile-every 5
#   (1 = every pass, N = every Nth, 0 = never; defaults live in moira.daemon.* of helixir.toml)
helixir merge --limit <n>              # run the NLI paraphrase backstop once (collective)
helixir journal | insights             # activity + insight journals (with provenance)
helixir watch start | run --once | stop | status   # Hygieia, the health watchdog:
#   DB liveness (self-heals via docker restart when allowed), container memory
#   pressure, orphaned daemons; alerts land as ops_alert notices IN the memory
helixir watch install | uninstall      # run the watchdog as a login service
#   (launchd / systemd user unit); watchdog.on_alert_cmd pushes each alert to
#   a human too — shell hook with HELIXIR_ALERT_KIND/_SUMMARY in the env
helixir health                         # recent health events (health.jsonl)
helixir config get | set <k> <v> | edit | apply   # the layered config, kubectl-style:
#   edit ~/.helixir/helixir.toml (comments preserved), validate, then `apply`
#   hot-reloads running MCP/gateway processes via SIGHUP — the client is rebuilt
#   from the re-read file and swapped atomically, no Claude Desktop reboot.
#   daemon/watch hold deeper snapshots and are listed as restart-to-apply.
#   get/get --raw and set confirmations redact every *_key, *_token,
#   *_password, *_secret, and *_credential field.
```

RBAC grants, groups, audit rows, and migration state live in HelixDB. The CLI
is only a management client over the same HQL contract used by MCP and the
library. See [Access control (RBAC)](#access-control-rbac) for the role matrix,
reserved workspaces, dedup federations, and trust boundary.

The `onboarding` membership is the admission event for new principals;
historical membership in either reserved workspace remains visible in the
registry. Administrators
can inspect the graph-derived registry and assign working groups without a
second policy store:

```bash
helixir rbac user list --json
helixir rbac user show --user alice --json
helixir rbac group add-user --group onboarding --user alice --role worker --json
helixir rbac group add-user --group development --user alice --role worker --json
helixir rbac group remove-user --group development --user alice --json
```

Removal deactivates assignments but retains the User node and role history.
The reserved `default`, `onboarding`, and membership-free `moirai` groups cannot
be deleted or placed in a dedup federation, and policy refuses to revoke its last global administrator.
When enforcement is enabled, management commands resolve the authenticated CLI
principal from `HELIXIR_RBAC_ACTOR`; there is intentionally no `--actor` escape
hatch. A global `admin` owns global and reserved policy; a `groupadmin` may list
and manage memberships and roles only inside its assigned non-reserved groups.

The gateway deliberately assumes a trusted network by default: it listens on
`gateway.default_bind` (`0.0.0.0:8765`) without authentication. To enable
bearer authentication, set `gateway.auth_token` with `helixir config set` or
provide `HELIXIR_GATEWAY_TOKEN`, then run `helixir config apply`. Tokens and
provider credentials are redacted from both resolved and raw config output.
`helixir gateway start --require-auth`
is the fail-closed variant: it serves `503` until a token is configured.

`helixir onboard` is the complete path: it provisions dependencies, bootstraps
RBAC, writes a stable per-client `HELIXIR_RBAC_ACTOR`, installs the canonical
Helixir Agent Skill, registers MCP non-destructively, and finishes with doctor.
`helixir setup` remains the lightweight MCP-only path.

## Integration

> The quickest path is **`helixir setup`** (above) — it detects your clients and writes the config for you. The manual JSON below is for reference or custom setups.

> **Make your agents *use* the memory well.** Wiring the MCP server is step one; the [`integration/`](integration/) templates (a drop-in `AGENTS.md` and a Claude `SKILLS.md`) encode how an agent should recall before answering, capture durable facts, and reason with FastThink — the same rules the maintainers run, so your agents get the same quality.

### Cursor

Add to `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "helixir": {
      "command": "/path/to/helixir-mcp",
      "env": {
        "HELIX_HOST": "localhost",
        "HELIX_PORT": "6969",
        "HELIX_LLM_PROVIDER": "cerebras",
        "HELIX_LLM_MODEL": "gpt-oss-120b",
        "HELIX_LLM_API_KEY": "YOUR_KEY",
        "HELIX_EMBEDDING_PROVIDER": "openai",
        "HELIX_EMBEDDING_MODEL": "nomic-embed-text-v1.5",
        "HELIX_EMBEDDING_URL": "https://openrouter.ai/api/v1",
        "HELIX_EMBEDDING_API_KEY": "YOUR_KEY"
      }
    }
  }
}
```

### Claude Desktop

**macOS:** `~/Library/Application Support/Claude/claude_desktop_config.json`
**Windows:** `%APPDATA%\Claude\claude_desktop_config.json`

Same JSON structure as above.

### Cursor Rules (recommended)

Add to **Cursor Settings > Rules** so the agent actually uses its memory:

```
# Core Memory Behavior
- At conversation start, call search_memory to recall relevant context
- After completing tasks, save key outcomes with add_memory
- Use search_by_concept for skill/preference/goal queries
- Use search_reasoning_chain for "why" questions

# FastThink for Complex Reasoning
- Before major decisions, use FastThink to structure your reasoning
- Flow: think_start -> think_add (repeat) -> think_recall -> think_conclude -> think_commit

# What to Save
- ALWAYS save: decisions, outcomes, architecture changes, error fixes, preferences
- NEVER save: grep results, lint output, file contents, temporary data
```

---

## Configuration

All settings are passed as environment variables.

### Required

| Variable | Description |
|:---------|:------------|
| `HELIX_HOST` | HelixDB address (default: `localhost`) |
| `HELIX_PORT` | HelixDB port (default: `6969`) |
| `HELIX_LLM_API_KEY` | API key for the LLM provider |
| `HELIX_EMBEDDING_API_KEY` | API key for the embedding provider |

### Optional

| Variable | Default | Description |
|:---------|:--------|:------------|
| `HELIXIR_MODE` | `solo` | Privilege tier: `solo` (private, no cross-user), `collective` (shared consensus), `insights` (+ generative Moirai) |
| `HELIX_LLM_PROVIDER` | `cerebras` | `cerebras`, `deepseek`, `ollama` |
| `HELIX_LLM_MODEL` | `gpt-oss-120b` | Model name; Cerebras is pinned to `gpt-oss-120b` |
| `HELIX_LLM_BASE_URL` | — | Custom endpoint (for Ollama or a self-hosted OpenAI-compatible API) |
| `HELIX_EMBEDDING_PROVIDER` | `openai` | `openai`, `ollama` |
| `HELIX_EMBEDDING_URL` | `https://openrouter.ai/api/v1` | Embedding API URL |
| `HELIX_EMBEDDING_MODEL` | `nomic-embed-text-v1.5` | Embedding model |
| `HELIX_LLM_FALLBACK_CHAIN` | `deepseek,ollama` | Ordered fallback tiers after the primary; empty value disables fallback |
| `HELIX_DEEPSEEK_API_KEY` | — | Credentials for the `deepseek` fallback tier |
| `RUST_LOG` | `helixir=warn` | Log level |

> **Automatic fallback chain.** When the primary LLM provider errors — a
> network outage *or* an exhausted quota — Helixir transparently retries the
> same request down an ordered chain, by default `deepseek → ollama`
> (smart remote → cheap remote → local selfhost), and readopts the primary as
> soon as it recovers. Tiers missing credentials are skipped at boot, so
> without a DeepSeek key the chain simply degrades to local Ollama
> (`llama3.2:3b` by default — the 2026-07 laptop bake-off winner: causal contract green at ~2x the speed and half the RAM of `qwen2.5:7b`). Tune via `llm_fallback_chain = ["deepseek",
> "ollama"]` + `deepseek_api_key` in `helixir.toml`, or the env vars above.

### Provider presets

<details>
<summary><b>Cerebras + OpenRouter</b> (recommended — fast inference, cheap embeddings)</summary>

```bash
HELIX_LLM_PROVIDER=cerebras
HELIX_LLM_MODEL=gpt-oss-120b
HELIX_LLM_API_KEY=csk-xxx           # https://cloud.cerebras.ai

HELIX_EMBEDDING_PROVIDER=openai
HELIX_EMBEDDING_URL=https://openrouter.ai/api/v1
HELIX_EMBEDDING_MODEL=nomic-embed-text-v1.5
HELIX_EMBEDDING_API_KEY=sk-or-xxx   # https://openrouter.ai/keys
```

</details>

<details>
<summary><b>DeepSeek + OpenRouter</b> (cheapest remote — ~$0.0001 per write)</summary>

```bash
HELIX_LLM_PROVIDER=deepseek
HELIX_LLM_MODEL=deepseek-v4-flash   # non-thinking mode is selected automatically
HELIX_LLM_API_KEY=sk-xxx            # https://platform.deepseek.com

HELIX_EMBEDDING_PROVIDER=openai
HELIX_EMBEDDING_URL=https://openrouter.ai/api/v1
HELIX_EMBEDDING_MODEL=nomic-embed-text-v1.5
HELIX_EMBEDDING_API_KEY=sk-or-xxx   # https://openrouter.ai/keys
```

</details>

<details>
<summary><b>Fully local with Ollama</b> (no API keys, fully private)</summary>

```bash
# Install Ollama: https://ollama.com
ollama pull llama3.2:3b
ollama pull nomic-embed-text

HELIX_LLM_PROVIDER=ollama
HELIX_LLM_MODEL=llama3.2:3b
HELIX_LLM_BASE_URL=http://localhost:11434

HELIX_EMBEDDING_PROVIDER=ollama
HELIX_EMBEDDING_URL=http://localhost:11434
HELIX_EMBEDDING_MODEL=nomic-embed-text
```

</details>

---

## Development

```bash
make build          # Build release binary
make test           # Run all tests
make check          # cargo check + clippy
make run            # Run MCP server locally (debug)
make deploy-schema  # Deploy schema to running HelixDB
make docker-up      # Start HelixDB container
make docker-down    # Stop HelixDB container
make test-e2e-hive  # Hive cross-user E2E (HelixDB + LLM + embeddings; set HELIX_* like MCP)
```

**Read-path E2E:** two suites guard retrieval quality and the LLM-free property — run them with a deliberately dead LLM key:

```bash
HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt HELIX_LLM_API_KEY=dead-key \
  cargo test -p helixir --test read_path_e2e -- --ignored --nocapture   # library level
HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt HELIX_LLM_API_KEY=dead-key \
  cargo test -p helixir --test mcp_read_e2e  -- --ignored --nocapture   # real MCP binary over stdio
```

**Hive E2E:** `make test-e2e-hive` runs `hive_cross_user_collective_link_e2e` (ignored by default in `cargo test`). It adds the same fact for two `user_id` values and asserts collective `user_count ≥ 2` on the first memory. LLM decisions can be flaky—retry if needed.

### Project structure

```
helixir-rs/
  helixir/
    src/
      bin/
        helixir.rs              # Thin CLI bootstrap/dispatch root
        helixir/                # Domain CLI modules (each <= 500 lines)
        helixir_mcp.rs          # MCP server entry point
        helixir_deploy.rs       # Schema deployment CLI
        helixir_bench.rs        # Latency bench + live probes (--chain/--add/--connect-probe)
      core/                     # Config, client, search modes
      db/                       # HelixDB client
      llm/                      # LLM providers, extractor, decision engine
      mcp/                      # MCP server, params, cognitive protocol
      toolkit/
        tooling_manager/        # Main pipeline (add, search, CRUD, events)
        mind_toolbox/           # Search engine, entity, ontology, reasoning
        fast_think/             # Working memory (petgraph-based)
    schema/
      schema.hx                 # Node/edge definitions (22 nodes + 5 vectors, 30 edges)
      queries.hx                # HQL queries (178)
    tests/                      # E2E suites: read_path (library) + mcp_read (stdio transport)
    memory-charter.md           # Write-path constitution: what may never be decided silently
    doc/                        # Engineering docs (architecture, dataflow, design rationale)
    Dockerfile
    docker-compose.yml
```

---

## License

[MIT](LICENSE) &copy; 2025-2026 Nikita Rulenko

## Links

- [HelixDB](https://github.com/HelixDB/helix-db) — graph + vector database
- [MCP Specification](https://modelcontextprotocol.io/) — Model Context Protocol
- [Cerebras](https://cloud.cerebras.ai) — fast LLM inference (free tier)
- [OpenRouter](https://openrouter.ai) — unified LLM/embedding API
