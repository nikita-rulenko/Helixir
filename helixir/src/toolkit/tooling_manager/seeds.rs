//! Self-seed: Helixir remembers what it is (issue #35).
//!
//! On startup (opt-in via `HELIXIR_SELF_SEED=1|blocking`) Helixir writes a
//! curated set of atomic facts about its own principles, charter and
//! operational gotchas under `user_id = "helixir"`. The set is versioned via
//! the `context_tags` marker (`helixir-seed@<N>`): seeding is idempotent and
//! re-runs only when `SEED_VERSION` is bumped, e.g. on upgrade.
//!
//! Seeds bypass LLM extraction on purpose — these facts must land verbatim.
//! Content derives from `doc/design-rationale.md`, `memory-charter.md` and
//! hard-won operational experience; it is a curated digest, not a docs dump.

use tracing::{info, warn};

use super::ToolingManager;
use crate::llm::extractor::ExtractedMemory;

/// Bump to re-seed on the next startup (old seeds stay — elder brain).
const SEED_VERSION: u32 = 2;

/// The system user that owns self-knowledge.
pub const SEED_USER: &str = "helixir";

fn seed_tag() -> String {
    format!("helixir-seed@{SEED_VERSION}")
}

/// `(importance, content)` — all seeds are ontology type `fact`,
/// certainty 95. Importance separates invariants (90) from gotchas (80).
const SEEDS: &[(i32, &str)] = &[
    // --- Identity & load-bearing invariants ---
    (
        90,
        "Helixir is an elder brain for LLM agents: a typed knowledge-graph memory on HelixDB that never deletes facts and reasons through multi-hop logical chains.",
    ),
    (
        90,
        "Helixir has no delete tool by design: outdated facts are superseded with history (HAS_HISTORY edges, valid_until) and remain reachable forever.",
    ),
    (
        90,
        "Memory nodes are shared across users: one fact is stored once and linked to each knower by a HasMemory edge; user_count tracks how many users know it.",
    ),
    (
        90,
        "All expensive work happens at write time (extraction, dedup decisions, relation inference); the read path makes zero LLM calls — the writer pays so the reader stays fast.",
    ),
    (
        90,
        "Long inputs are preserved verbatim as a Memory with source=raw_input alongside extracted facts; raw_input memories must never be modified or superseded.",
    ),
    (
        90,
        "The Helixir ontology has exactly 8 fixed types: fact, preference, skill, goal, opinion, experience, achievement, action; the list is static by design.",
    ),
    (
        90,
        "Reasoning relations BECAUSE, IMPLIES, SUPPORTS and CONTRADICTS are first-class graph edges, not metadata; reasoning chains and connect_memories traverse them.",
    ),
    (
        90,
        "Temporal windows and decay govern attention (search entry points), never reachability: graph traversal pulls connected facts from any era.",
    ),
    // --- Memory charter ---
    (
        90,
        "The memory charter (memory-charter.md) lists conflicts Helixir may never resolve silently; they are returned in add_memory.needs_clarification for the agent to escalate to the human.",
    ),
    (
        85,
        "Memory charter C1: memory never deletes itself silently — DELETE decisions always escalate to the agent.",
    ),
    (
        85,
        "Memory charter C3: preferences, goals and opinions are never rewritten silently, even when the decision engine is highly confident.",
    ),
    (
        85,
        "Memory charter C5: low-confidence UPDATE or SUPERSEDE decisions (confidence below 70) escalate for review.",
    ),
    // --- Operational gotchas (hard-won) ---
    (
        80,
        "HelixDB builds its BM25 index on insert; enabling bm25=true later does not retroactively index existing data. A full rebuild runs at startup only when the stored BM25 schema_version stamp differs from the binary's version.",
    ),
    (
        80,
        "HelixDB search visibility can lag writes because gateway workers hold read snapshots; re-probe before concluding that data is missing.",
    ),
    (
        80,
        "A HelixDB lock.mdb created inside a Linux container cannot be reused by host macOS processes (EINVAL on write); move it aside only while the container is stopped.",
    ),
    (
        80,
        "HelixDB errors often arrive with HTTP 200; always check the response body for an error field instead of trusting the status code.",
    ),
    (
        80,
        "To upgrade HelixDB: run helix update for the CLI, then helix push <instance> from the workspace that owns the instance; archive the instance data volume first.",
    ),
    (
        80,
        "MCP clients such as Claude Code cache server env at session start; changes to mcpServers env do not reach respawned servers until the client itself restarts.",
    ),
    (
        80,
        "HELIXIR_RETRIEVAL_PROFILE=algo_opt enables the optimized read path (BM25 hybrid via RRF, batched graph expansion, PPR ranking, provenance, LLM-free chains); the default legacy profile preserves historic behaviour bit-for-bit.",
    ),
    (
        80,
        "HELIXIR_EMBED_CACHE_PATH enables the persistent embedding cache and HELIXIR_EMBED_CACHE_WARMUP pre-embeds the corpus at startup, eliminating cold-start re-embedding.",
    ),
    (
        80,
        "The Helixir crate MSRV is Rust 1.85; let-chain syntax must not be used.",
    ),
    (
        80,
        "The e2e suites read_path_e2e and mcp_read_e2e run with HELIX_E2E=1 and a deliberately dead LLM key, proving the read path needs no LLM.",
    ),
    (
        80,
        "helixir-bench provides live debug probes: --chain-probe, --add-probe and --connect-probe with --query-b.",
    ),
    // --- Read surface facts ---
    (
        85,
        "search_memory results carry provenance metadata: origin (seed or graph), edge, parent, depth and ppr — the agent can verify why each fact was returned.",
    ),
    (
        85,
        "connect_memories(query_a, query_b) finds the path between two concepts via bidirectional BFS and reports edge types with cumulative confidence (the product of edge weights).",
    ),
    (
        80,
        "Under algo_opt the final ranking blends 0.3 cosine similarity, 0.5 Personalized PageRank mass and 0.2 temporal freshness.",
    ),
    // --- Operations & integration (seeded at install: the manual lives inside the memory) ---
    (
        90,
        "Helixir runs in three modes of escalating trust: solo (one agent, one user_id, private dedup), collective (write-time dedup across all user_ids; contradictions are scored by stances and user_count consensus, never resolved by rewriting the graph) and insights (the Moirai — Clotho, Lachesis, Atropos — generate tag dictionaries, indirect multi-hop correlations and curated hypotheses). Set mode in ~/.helixir/helixir.toml (mode = \"Solo|Collective|Insights\") or the HELIXIR_MODE env var; env overrides the toml.",
    ),
    (
        85,
        "The helixir CLI drives the generative layer: clotho grow --user <id> grows the tag dictionary; pipeline --user <id> runs one Clotho-Lachesis-Atropos pass; daemon start --user <id> --interval <secs> runs passes continuously with per-stage cadence flags --clotho-every, --insight-every, --merge-every, --reconcile-every (1 = every pass, 0 = never); categories, swarm, heartbeat, merge, insights and journal inspect and drive the rest.",
    ),
    (
        85,
        "Moirai intensity thresholds live in ~/.helixir/helixir.toml: [moira.clotho] grow_threshold and tag_threshold, [moira.lachesis] subset_pmi_bar, coherence_bar and dfs_budget, [moira.atropos] quality_pmi_bar (curation strictness), [moira.daemon] interval_secs and the *_every_passes cadences.",
    ),
    (
        85,
        "To connect Claude Desktop or Claude Code, add an mcpServers entry named helixir-local running the helixir-mcp binary with HELIX_HOST/HELIX_PORT/HELIX_INSTANCE plus LLM and embedding provider env; helixir setup writes this non-destructively. For Cursor use ~/.cursor/mcp.json with the same env. For network clients, helixir gateway start serves the same MCP tools over streamable-HTTP at /mcp.",
    ),
    (
        85,
        "To connect zeroclaw, add a [[mcp.servers]] stdio entry named helixir-local in ~/.zeroclaw/config.toml pointing at the helixir-mcp binary with the HELIX_* env in [mcp.servers.env]; its tools register as helixir-local__<tool> and are deferred, so autonomy.auto_approve must include tool_search plus the helixir-local__* tool names for non-interactive runs.",
    ),
    (
        90,
        "Agents must establish identity BEFORE the first recall: use the assigned user_id; otherwise derive a stable one from the agent's own name, and consult list_users when unsure — never adopt another agent's id (the example id claude in templates is a placeholder to replace).",
    ),
    (
        90,
        "Agent usage rules: recall first at the start of any non-trivial task and right after a context summary (retry with scope=collective if personal recall is empty); capture durable facts proactively (decisions, preferences, goals, constraints, outcomes, gotchas — never secrets or ephemeral chatter); surface needs_clarification questions to the human instead of resolving conflicts silently.",
    ),
    (
        90,
        "Every add_memory result carries a top-level ok field: ok true means the write succeeded (memory_ids inline, or status accepted for a buffered write still finishing) — never retry on ok true; deduped entries with memories_added 0 mean already known, which is success; only ok false is a failure.",
    ),
    (
        85,
        "For multi-step reasoning use FastThink: think_start, then think_add steps, optional think_recall to pull known facts, think_conclude, and ONE think_commit at the end — it synthesizes the whole session into a single enriched memory and is the heaviest call; think_discard throws the scratchpad away.",
    ),
    (
        85,
        "Data safety: the LMDB data lives wherever the HelixDB container's data dir is bind-mounted — back it up before schema deploys; schema changes are compiled into the server (no hot reload), deploy = helix check, rebuild the image, swap the container onto the same volume; HelixDB errors often hide in HTTP-200 response bodies, and search visibility lags writes, so re-probe before concluding data is missing.",
    ),
    (
        80,
        "The drop-in agent templates live in the repo's integration folder: AGENTS.md for any coding agent via the agents.md convention and SKILLS.md for Claude as a skill — both encode the recall-capture-reason loop so every connected agent uses the memory the way its maintainers do.",
    ),
];

impl ToolingManager {
    /// Idempotent self-seed, gated by `HELIXIR_SELF_SEED=1` (background) or
    /// `=blocking` (await — for tests and first-run setup scripts).
    pub(crate) async fn maybe_seed_system_memories(&self) {
        let mode = std::env::var("HELIXIR_SELF_SEED").unwrap_or_default();
        let mode = mode.trim().to_ascii_lowercase();
        if mode.is_empty() || mode == "0" || mode == "false" {
            return;
        }
        // ~26 facts = one embedding batch + the inserts (a couple of
        // seconds, once per version) — cheap enough to run inline.
        let _ = mode;
        self.seed_system_memories().await;
    }

    async fn seed_system_memories(&self) {
        let tag = seed_tag();

        if self.seed_version_present(&tag).await {
            info!("Self-seed: {tag} already present, skipping");
            return;
        }

        info!("Self-seed: writing {} system facts ({tag})", SEEDS.len());
        let texts: Vec<&str> = SEEDS.iter().map(|(_, t)| *t).collect();
        let embeddings = match self.embedder.generate_batch(&texts, true).await {
            Ok(e) => e,
            Err(e) => {
                warn!("Self-seed: embedding failed, skipping: {e}");
                return;
            }
        };

        let mut stored = 0usize;
        for ((importance, text), vector) in SEEDS.iter().zip(embeddings.iter()) {
            let memory = ExtractedMemory {
                text: (*text).to_string(),
                memory_type: "fact".to_string(),
                certainty: 95,
                importance: *importance,
                entities: vec![],
                context: None,
            };
            match self
                .store_new_memory(&memory, SEED_USER, vector, &tag)
                .await
            {
                Ok(_) => stored += 1,
                Err(e) => warn!("Self-seed: failed to store a seed: {e}"),
            }
        }
        info!("Self-seed: stored {stored}/{} facts ({tag})", SEEDS.len());
    }

    async fn seed_version_present(&self, tag: &str) -> bool {
        #[derive(serde::Deserialize)]
        struct MemoriesResponse {
            #[serde(default)]
            memories: Vec<SeedProbe>,
        }
        #[derive(serde::Deserialize)]
        struct SeedProbe {
            #[serde(default)]
            context_tags: String,
        }

        let params = serde_json::json!({ "user_id": SEED_USER, "limit": 1000 });
        match self
            .db
            .execute_query::<MemoriesResponse, _>("getUserMemories", &params)
            .await
        {
            Ok(r) => r.memories.iter().any(|m| m.context_tags == tag),
            // "No value found" for a fresh instance is expected — seed away.
            Err(_) => false,
        }
    }
}
