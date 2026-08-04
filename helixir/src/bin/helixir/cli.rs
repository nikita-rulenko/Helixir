use super::*;

#[derive(Parser)]
#[command(
    name = "helixir",
    version,
    about = "Helixir agent control & monitoring (the Moirai)"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) cmd: Cmd,
}

#[derive(Args, Debug, Clone, Default)]
pub(crate) struct OnboardModelArgs {
    /// Exact Ollama LLM model to pull and configure.
    #[arg(long, value_name = "MODEL", conflicts_with = "no_local_llm")]
    pub(crate) local_llm_model: Option<String>,
    /// Do not provision a local fallback LLM.
    #[arg(long, conflicts_with = "local_llm_model")]
    pub(crate) no_local_llm: bool,
    /// Use an explicitly configured remote embedding service instead of Ollama/Nomic.
    #[arg(long)]
    pub(crate) remote_embeddings: bool,
    /// Remote embedding adapter (`openai` for OpenAI-compatible APIs).
    #[arg(long, value_name = "PROVIDER", requires = "remote_embeddings")]
    pub(crate) embedding_provider: Option<String>,
    /// Remote embedding model name.
    #[arg(long, value_name = "MODEL", requires = "remote_embeddings")]
    pub(crate) embedding_model: Option<String>,
    /// Remote OpenAI-compatible API root.
    #[arg(long, value_name = "URL", requires = "remote_embeddings")]
    pub(crate) embedding_url: Option<String>,
}

#[derive(Args, Debug, Clone, Default)]
pub(crate) struct OnboardSecurityArgs {
    /// Keep the historical RBAC-disabled trusted-network mode.
    #[arg(long)]
    pub(crate) legacy_trusted_mode: bool,
    /// Initial global administrator id.
    #[arg(long, value_name = "ID")]
    pub(crate) rbac_operator: Option<String>,
    /// Additional onboarding group principal; repeat for multiple agents.
    #[arg(long = "rbac-principal", value_name = "ID")]
    pub(crate) rbac_principals: Vec<String>,
}

#[derive(Subcommand)]
pub(crate) enum Cmd {
    /// Show, edit, validate and hot-apply the layered config (#52)
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// Memory charter review: adopted learned rules + precedent counts (#34).
    Charter,
    /// Delete an Agent presence row (#84, operator-only): for true junk —
    /// test agents, renamed identities. Stale agents are already flagged.
    PruneAgent {
        #[arg(long)]
        agent_id: String,
        /// Confirm the deletion (refuses without it).
        #[arg(long)]
        yes: bool,
    },
    /// List categories with member counts (tag coverage / subset sizes).
    Categories {
        #[arg(long, default_value_t = 500)]
        limit: i64,
    },
    /// Clotho — the Spinner (tagging agent).
    Clotho {
        #[command(subcommand)]
        cmd: ClothoCmd,
    },
    /// Lachesis — the Measurer (routing + apophenia gate).
    Lachesis {
        #[command(subcommand)]
        cmd: LachesisCmd,
    },
    /// Longest coherent reasoning chain through a topic (#47).
    Chain {
        #[arg(long)]
        user: String,
        #[arg(long)]
        topic: String,
        #[arg(long = "max-hops", default_value_t = 8)]
        max_hops: usize,
    },
    /// Show recent agent activity from the journal.
    Journal {
        #[arg(long, default_value_t = 20)]
        tail: usize,
    },
    /// Atropos — curate Lachesis threads into ranked insights + journal them.
    Atropos {
        #[arg(long, default_value_t = 200)]
        limit: i64,
        #[arg(long = "max-seeds", default_value_t = 24)]
        max_seeds: usize,
        #[arg(long = "max-hops", default_value_t = 5)]
        max_hops: usize,
    },
    /// Show the insight journal (Atropos output).
    Insights {
        #[arg(long, default_value_t = 15)]
        tail: usize,
    },
    /// Contradiction debt — open cross-user disputes; `--reconcile` drains the
    /// dead ones (preferences coexist; live factual disputes are kept) (#45).
    Debt {
        #[arg(long)]
        user: String,
        #[arg(long, default_value_t = 500)]
        limit: i64,
        #[arg(long)]
        reconcile: bool,
    },
    /// Backfill content_key fingerprints onto existing memories (#43 migration).
    /// Idempotent — already-keyed nodes are skipped, safe to re-run.
    Backfill {
        #[arg(long, default_value_t = 100000)]
        limit: i64,
    },
    /// Paraphrase backstop (#43/#55): merge facts that mean the same but are
    /// worded differently by unifying their fingerprint. NLI-gated — never merges
    /// contradictions. Needs the local NLI model (`helixir model download`).
    Merge {
        #[arg(long, default_value_t = 500)]
        limit: i64,
        /// Cosine pre-filter; pairs below this aren't even shown to the judge.
        #[arg(long, default_value_t = 0.85)]
        threshold: f64,
    },
    /// Manage the local NLI model (#55) — the contradiction-safe judge for
    /// paraphrase merging. The repo ships only the downloader; it fetches the
    /// ONNX variant matching your CPU/OS on demand (~90 MB). Used by the
    /// collective/insights tiers.
    Model {
        #[command(subcommand)]
        sub: ModelCmd,
    },
    /// Run the full orchestrated pass over a user: Clotho → Lachesis → Atropos.
    Pipeline {
        #[arg(long)]
        user: String,
        #[arg(long, default_value_t = 0.62)]
        threshold: f64,
        #[arg(long = "max-seeds", default_value_t = 24)]
        max_seeds: usize,
        #[arg(long = "max-hops", default_value_t = 5)]
        max_hops: usize,
    },
    /// Swarm roster — every agent the collective knows, live ones first (#39).
    /// The rendezvous is the shared DB, not CLI-to-CLI: any host's agents appear.
    Swarm {
        /// Heartbeats within this many seconds count as active.
        /// Defaults to `swarm.active_window_secs` from config.
        #[arg(long)]
        window: Option<u64>,
    },
    /// Announce this agent's presence to the collective (one heartbeat).
    Heartbeat {
        #[arg(long)]
        agent: String,
        #[arg(long, default_value = "developer")]
        role: String,
        /// Host label; blank → $HELIXIR_HOST_LABEL / $HOSTNAME / $HOST / "unknown".
        #[arg(long, default_value = "")]
        host: String,
        #[arg(long, default_value = "idle")]
        status: String,
    },
    /// The per-host MCP gateway (#42): serve the same memory tools over HTTP
    /// (streamable-http) so many clients share one process — they point at the
    /// gateway URL instead of each spawning a stdio helixir-mcp. Foreground or
    /// background. Bearer authentication is optional and disabled by default.
    Gateway {
        #[command(subcommand)]
        cmd: GatewayCmd,
    },
    /// The Moira daemon — schedule full passes (foreground or background).
    Daemon {
        #[command(subcommand)]
        cmd: DaemonCmd,
    },
    /// Hygieia — the health watchdog: DB liveness, container memory,
    /// orphaned daemons; self-heals where allowed, alerts through the memory.
    Watch {
        #[command(subcommand)]
        cmd: WatchCmd,
    },
    /// Recent health events (Hygieia's journal, ~/.helixir/health.jsonl).
    Health {
        #[arg(long, default_value_t = 20)]
        tail: usize,
    },
    /// Configure Helixir + wire its MCP server into your agent clients
    /// (Claude Code, Claude Desktop, Cursor, Gemini CLI).
    Setup {
        /// Skip prompts: use HELIX_* env + defaults, wire all detected clients.
        #[arg(long = "non-interactive")]
        non_interactive: bool,
        /// Show what would be written without changing anything.
        #[arg(long = "dry-run")]
        dry_run: bool,
        /// Wire this exact config file instead of auto-detecting clients.
        #[arg(long)]
        target: Option<String>,
        /// Wire clients to a per-host GATEWAY over HTTP instead of spawning a
        /// stdio helixir-mcp. Accepts a URL or host:port (→ http://host:port/mcp).
        /// Clients then carry no HELIX_* env — just the gateway URL.
        #[arg(long)]
        gateway: Option<String>,
        /// Privilege tier to write (solo | collective | insights). When omitted,
        /// setup recommends `collective` (shared memory — the point of the tool);
        /// pass `--mode solo` for private, single-user memory. The silent library
        /// default (no setup) stays solo.
        #[arg(long)]
        mode: Option<String>,
    },
    /// Build and apply the guided installation plan.
    Onboard {
        /// Skip prompts and use HELIX_* values plus deterministic defaults.
        #[arg(long = "non-interactive")]
        non_interactive: bool,
        /// Print the plan without applying platform changes.
        #[arg(long = "dry-run")]
        dry_run: bool,
        /// Privilege tier to provision (solo | collective | insights).
        #[arg(long)]
        mode: Option<String>,
        /// Deterministic local-model choices shared with interactive onboarding.
        #[command(flatten)]
        models: OnboardModelArgs,
        /// RBAC-by-default security profile choices.
        #[command(flatten)]
        security: OnboardSecurityArgs,
    },
    /// Readiness report; repairs broken embeddings with Ollama/Nomic.
    Doctor {
        /// Emit a stable machine-readable JSON report.
        #[arg(long)]
        json: bool,
    },
    /// Show the current privilege tier (HELIXIR_MODE) and what it permits.
    Mode,
    /// Manage HelixDB-backed role assignments and groups.
    Rbac {
        #[command(subcommand)]
        cmd: RbacCmd,
    },
}
