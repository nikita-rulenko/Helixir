use super::*;

#[derive(Subcommand)]
pub(crate) enum ControlPlaneCmd {
    /// Install the reboot-safe supervisor service and hardened web container.
    Install {
        /// Immutable control-plane image to run.
        #[arg(long)]
        image: Option<String>,
    },
    /// Report supervisor and container lifecycle state.
    Status,
    /// Remove the web container and supervisor login service.
    Uninstall,
}

#[derive(Subcommand)]
pub(crate) enum RbacCmd {
    /// Converge the permanent default/onboarding/Moirai RBAC workspaces.
    Bootstrap {
        /// Global operator id. Defaults to HELIXIR_RBAC_ACTOR or the OS user.
        #[arg(long)]
        operator: Option<String>,
        /// Principal to admit; repeat for multiple MCP clients.
        #[arg(long = "principal")]
        principals: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show the persisted RBAC state.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Explicitly replace legacy teamlead assignments with groupadmin.
    MigrateTeamleads {
        /// Confirm the privilege-changing migration.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    /// Manage named groups.
    Group {
        #[command(subcommand)]
        cmd: RbacGroupCmd,
    },
    /// Inspect the graph-backed user/agent registry.
    User {
        #[command(subcommand)]
        cmd: RbacUserCmd,
    },
    /// Manage federated deduplication and shared group visibility.
    Dedup {
        #[command(subcommand)]
        cmd: RbacDedupCmd,
    },
    /// Grant a global or group-scoped role.
    Grant {
        #[arg(long)]
        user: String,
        #[arg(long)]
        role: String,
        #[arg(long)]
        group: Option<String>,
    },
    /// Revoke an existing role assignment (audit row is retained).
    Revoke {
        #[arg(long)]
        user: String,
        #[arg(long)]
        role: String,
        #[arg(long)]
        group: Option<String>,
    },
    /// Print roles for one user, or all active assignments.
    Show {
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Check whether a principal can read or write a memory owner.
    Check {
        #[arg(long)]
        user: String,
        #[arg(long, default_value = "read")]
        action: String,
        #[arg(long)]
        owner: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum RbacGroupCmd {
    /// Create or update a group.
    Create {
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "")]
        description: String,
    },
    /// List active groups.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Add a registered or new user to this group.
    AddUser {
        #[arg(long)]
        group: String,
        #[arg(long)]
        user: String,
        #[arg(long, default_value = "worker")]
        role: String,
        #[arg(long)]
        json: bool,
    },
    /// Remove every active role for a user in this group; history is retained.
    RemoveUser {
        #[arg(long)]
        group: String,
        #[arg(long)]
        user: String,
        #[arg(long)]
        json: bool,
    },
    /// Deactivate a group; grants remain in the audit history.
    Delete {
        #[arg(long)]
        id: String,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum RbacUserCmd {
    /// List registered users, active roles, role history, and agent presence.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show one registered user.
    Show {
        #[arg(long)]
        user: String,
        #[arg(long)]
        json: bool,
    },
    /// Complete a remote client's placement into a working workspace.
    Onboard {
        /// Principal previously admitted through reserved onboarding.
        #[arg(long)]
        user: String,
        /// Existing target group, or a new id when --group-name is supplied.
        #[arg(long)]
        group: String,
        /// Human-readable name used only when the target group is missing.
        #[arg(long)]
        group_name: Option<String>,
        /// Description used only when creating the target group.
        #[arg(long, default_value = "")]
        description: String,
        /// Group-scoped role: groupadmin, moderator, worker, or viewer.
        #[arg(long, default_value = "worker")]
        role: String,
        /// Retain the temporary onboarding membership after assignment.
        #[arg(long)]
        keep_onboarding: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum RbacDedupCmd {
    /// Create or update a dedup federation.
    Create {
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "")]
        description: String,
    },
    /// List active dedup federations and their current groups.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Join a group and grant it the federation's existing memory history.
    Attach {
        #[arg(long)]
        group: String,
        #[arg(long = "dedup-group")]
        dedup_group: String,
    },
    /// Leave prospectively; historical memory access is retained.
    Detach {
        #[arg(long)]
        group: String,
    },
    /// Deactivate an empty dedup federation.
    Delete {
        #[arg(long)]
        id: String,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum ConfigCmd {
    /// Print the RESOLVED config (defaults -> helixir.toml -> env)
    Get {
        /// Print the raw helixir.toml file instead of the resolved view
        #[arg(long)]
        raw: bool,
    },
    /// Set one key in helixir.toml (dotted path, e.g. watchdog.mem_restart_pct 90)
    Set { key: String, value: String },
    /// Open helixir.toml in $EDITOR, then validate it
    Edit,
    /// Validate helixir.toml and hot-reload running processes (kubectl-apply style)
    Apply,
}

#[derive(Subcommand)]
pub(crate) enum ModelCmd {
    /// Download the NLI model variant for THIS machine (arch/CPU-aware), ~90 MB,
    /// into ~/.helixir/models/nli. Skips files already present unless --force.
    Download {
        /// Re-download even if the files are already present.
        #[arg(long)]
        force: bool,
    },
    /// Show what's installed and which variant fits this host.
    Status,
    /// Liveness + readiness check: load the model and classify canonical pairs,
    /// proving it detects contradictions (never merges opposites) and paraphrases.
    Check,
    /// Print which ONNX variant would be downloaded for this host (no download).
    Which,
}

#[derive(Subcommand)]
pub(crate) enum WatchCmd {
    /// Run the watchdog loop in the FOREGROUND (Ctrl-C to stop; or --once).
    Run {
        /// One sampling tick, then exit (for smoke tests and cron).
        #[arg(long)]
        once: bool,
        /// Sampling period in seconds. Default: config watchdog.sample_interval_secs.
        #[arg(long)]
        interval: Option<u64>,
    },
    /// Start a DETACHED background watchdog. Writes a PID file; `stop` ends it.
    Start {
        #[arg(long)]
        interval: Option<u64>,
    },
    /// SIGTERM the background watchdog.
    Stop,
    /// Is the background watchdog alive?
    Status,
    /// Install the watchdog as a login service (launchd on macOS, systemd
    /// user unit on Linux) so it survives reboots (#75).
    Install,
    /// Remove the login service installed by `watch install`.
    Uninstall,
}

#[derive(Subcommand)]
pub(crate) enum DaemonCmd {
    /// Run in the FOREGROUND (loop on the interval, Ctrl-C to stop; or --once).
    Run {
        #[arg(long)]
        user: String,
        #[arg(long, default_value_t = 300)]
        interval: u64,
        #[arg(long)]
        once: bool,
        #[arg(long, default_value_t = 0.62)]
        threshold: f64,
        #[arg(long = "max-seeds", default_value_t = 24)]
        max_seeds: usize,
        #[arg(long = "max-hops", default_value_t = 5)]
        max_hops: usize,
        /// Run Clotho (tagging) every Nth pass (0 = never). Default: config.
        #[arg(long = "clotho-every")]
        clotho_every: Option<u64>,
        /// Run the insight stage (Lachesis routing + Atropos curation) every
        /// Nth pass (0 = never). Default: config.
        #[arg(long = "insight-every")]
        insight_every: Option<u64>,
        /// Run the NLI paraphrase merge every Nth pass (0 = never). Default: config.
        #[arg(long = "merge-every")]
        merge_every: Option<u64>,
        /// Drain contradiction debt every Nth pass (0 = never). Default: config.
        #[arg(long = "reconcile-every")]
        reconcile_every: Option<u64>,
    },
    /// Start a DETACHED background daemon (a frequency implies it should keep
    /// running). Writes a PID file; `stop` ends it.
    Start {
        #[arg(long)]
        user: String,
        #[arg(long, default_value_t = 300)]
        interval: u64,
        #[arg(long, default_value_t = 0.62)]
        threshold: f64,
        #[arg(long = "max-seeds", default_value_t = 24)]
        max_seeds: usize,
        #[arg(long = "max-hops", default_value_t = 5)]
        max_hops: usize,
        /// Run Clotho (tagging) every Nth pass (0 = never). Default: config.
        #[arg(long = "clotho-every")]
        clotho_every: Option<u64>,
        /// Run the insight stage (Lachesis routing + Atropos curation) every
        /// Nth pass (0 = never). Default: config.
        #[arg(long = "insight-every")]
        insight_every: Option<u64>,
        /// Run the NLI paraphrase merge every Nth pass (0 = never). Default: config.
        #[arg(long = "merge-every")]
        merge_every: Option<u64>,
        /// Drain contradiction debt every Nth pass (0 = never). Default: config.
        #[arg(long = "reconcile-every")]
        reconcile_every: Option<u64>,
    },
    /// Stop the background daemon.
    Stop,
    /// Show the background daemon's status.
    Status,
}

#[derive(Subcommand)]
pub(crate) enum GatewayCmd {
    /// Run in the FOREGROUND (serve until Ctrl-C).
    Run {
        /// Listen address. Defaults to `gateway.default_bind` from config.
        #[arg(long)]
        bind: Option<String>,
        /// Refuse all requests if no gateway token is configured.
        #[arg(long)]
        require_auth: bool,
    },
    /// Start a DETACHED background gateway. Writes a PID file; `stop` ends it.
    Start {
        /// Listen address. Defaults to `gateway.default_bind` from config.
        #[arg(long)]
        bind: Option<String>,
        /// Refuse all requests if no gateway token is configured.
        #[arg(long)]
        require_auth: bool,
    },
    /// Stop the background gateway.
    Stop,
    /// Show the background gateway's status.
    Status,
}

#[derive(Subcommand)]
pub(crate) enum ClothoCmd {
    /// Seed the controlled category dictionary (idempotent).
    Seed,
    /// Auto-tag a user's memories — point Clotho at the real corpus.
    Tag {
        #[arg(long)]
        user: String,
        #[arg(long, default_value_t = 500)]
        limit: i64,
        #[arg(long, default_value_t = 0.65)]
        threshold: f64,
        #[arg(long = "top-k", default_value_t = 5)]
        top_k: i64,
    },
    /// Grow-and-tag: match against the live dictionary, mint a category via the
    /// LLM on a miss — the dictionary self-builds from the corpus.
    Grow {
        #[arg(long)]
        user: String,
        #[arg(long, default_value_t = 200)]
        limit: i64,
        #[arg(long, default_value_t = 0.62)]
        threshold: f64,
    },
}

#[derive(Subcommand)]
pub(crate) enum LachesisCmd {
    /// PMI link strength between two categories (by category_id).
    Pmi {
        cat_a: String,
        cat_b: String,
        #[arg(long)]
        universe: Option<usize>,
    },
    /// Route a cross-domain subset thread from a seed category (by category_id).
    Route {
        #[arg(long)]
        seed: String,
        #[arg(long)]
        universe: Option<usize>,
        #[arg(long = "max-hops", default_value_t = 5)]
        max_hops: usize,
    },
}

// `mode_gate` in the adjacent module enforces privilege-tier availability.
