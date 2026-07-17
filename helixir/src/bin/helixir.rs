//! `helixir` — the agent control & monitoring CLI (Moirai).
//!
//! The Moirai are background agents; at this stage the CLI is the dashboard that
//! both *drives* them (seed/tag/route) and *observes* them — live `tracing` logs
//! on stderr plus a persistent **activity journal** (append-only JSONL). When the
//! daemon (#42) lands and the agents run continuously, it writes to the same
//! journal and `helixir journal` becomes the monitor for the background fleet.
//!
//! Config comes from the same `HELIX_*` env as the MCP server (`from_env`).
//! Journal path: `$HELIXIR_AGENT_LOG` or `./helixir-agent-activity.jsonl`.

use std::fs::OpenOptions;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use dialoguer::{Confirm, Input, MultiSelect, Select};
use helixir::HelixClient;
use helixir::agents::atropos::Insight;
use helixir::agents::daemon::DaemonConfig;
use helixir::agents::orchestrator::PassConfig;
use helixir::core::HelixirClient;
use helixir::core::config::MemoryMode;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "helixir",
    version,
    about = "Helixir agent control & monitoring (the Moirai)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
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
    /// Show the current privilege tier (HELIXIR_MODE) and what it permits.
    Mode,
}

#[derive(Subcommand)]
enum ConfigCmd {
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
enum ModelCmd {
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
enum WatchCmd {
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
enum DaemonCmd {
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
enum GatewayCmd {
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
enum ClothoCmd {
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
enum LachesisCmd {
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

/// Refuse commands the current privilege tier doesn't permit. Generative
/// commands need `insights`; collective-surface commands need `collective`.
/// Reads, lifecycle, setup, and the journal views are always allowed.
fn mode_gate(cmd: &Cmd, mode: MemoryMode) -> Result<()> {
    let needs_insights = matches!(
        cmd,
        Cmd::Clotho { .. }
            | Cmd::Lachesis { .. }
            | Cmd::Atropos { .. }
            | Cmd::Pipeline { .. }
            | Cmd::Daemon {
                cmd: DaemonCmd::Run { .. }
            }
    );
    let needs_collective = matches!(
        cmd,
        Cmd::Swarm { .. } | Cmd::Heartbeat { .. } | Cmd::Debt { .. }
    );
    if needs_insights && !mode.insights_enabled() {
        anyhow::bail!(
            "`{}` needs HELIXIR_MODE=insights (current: {}); the generative Moirai are off by default",
            cmd_name(cmd),
            mode.label()
        );
    }
    if needs_collective && !mode.collective_enabled() {
        anyhow::bail!(
            "`{}` needs HELIXIR_MODE=collective or insights (current: {}); cross-user features are off by default",
            cmd_name(cmd),
            mode.label()
        );
    }
    Ok(())
}

// ============ helixir config (#52) ============

/// The file `config set/edit/apply` operates on: the resolved existing file,
/// else `~/.helixir/helixir.toml` (created on first `set`).
fn config_target_path() -> Result<PathBuf> {
    if let Some(p) = helixir::core::config::HelixirConfig::config_file_path() {
        return Ok(p);
    }
    Ok(helixir_dir()?.join("helixir.toml"))
}

fn config_get(raw: bool) -> Result<()> {
    if raw {
        let p = config_target_path()?;
        match std::fs::read_to_string(&p) {
            Ok(s) => {
                let mut doc: toml_edit::DocumentMut = s.parse().context("parse helixir.toml")?;
                if let Some(token) = doc
                    .get_mut("gateway")
                    .and_then(toml_edit::Item::as_table_mut)
                    .and_then(|gateway| gateway.get_mut("auth_token"))
                {
                    *token = toml_edit::value("<redacted>");
                }
                print!("{doc}");
            }
            Err(_) => println!(
                "# {} does not exist — everything is at defaults",
                p.display()
            ),
        }
        return Ok(());
    }
    let mut resolved = helixir::core::config::HelixirConfig::from_env();
    if resolved.gateway.auth_token.is_some() {
        resolved.gateway.auth_token = Some("<redacted>".to_string());
    }
    println!(
        "# RESOLVED config: defaults -> helixir.toml -> env (env wins)\n{}",
        toml::to_string_pretty(&resolved).context("serialize resolved config")?
    );
    Ok(())
}

/// Validate a helixir.toml body the same way the loader consumes it.
fn config_validate(content: &str) -> Result<()> {
    toml::from_str::<helixir::core::config::HelixirConfig>(content)
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("invalid helixir.toml: {e}"))
}

fn config_set(key: &str, value: &str) -> Result<()> {
    let p = config_target_path()?;
    let content = std::fs::read_to_string(&p).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = content.parse().context("parse helixir.toml")?;

    let segs: Vec<&str> = key.split('.').collect();
    anyhow::ensure!(!segs.is_empty(), "empty key");
    let mut node = doc.as_table_mut();
    for seg in &segs[..segs.len() - 1] {
        node = node
            .entry(seg)
            .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("{seg} exists and is not a table"))?;
    }
    // Try native TOML typing first (5, 5.0, true, [..]); fall back to string.
    let item: toml_edit::Value = value
        .parse()
        .unwrap_or_else(|_| toml_edit::Value::from(value));
    node[segs[segs.len() - 1]] = toml_edit::Item::Value(item);

    let out = doc.to_string();
    config_validate(&out)?; // never persist a file the loader would reject
    std::fs::write(&p, out).with_context(|| format!("write {}", p.display()))?;
    let displayed_value = if key == "gateway.auth_token" {
        "<redacted>"
    } else {
        value
    };
    println!("{} = {} -> {}", key, displayed_value, p.display());
    println!("run `helixir config apply` to hot-reload running processes");
    Ok(())
}

fn config_edit() -> Result<()> {
    let p = config_target_path()?;
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = std::process::Command::new(&editor).arg(&p).status()?;
    anyhow::ensure!(status.success(), "{editor} exited with {status}");
    match std::fs::read_to_string(&p) {
        Ok(content) => match config_validate(&content) {
            Ok(()) => println!("valid — run `helixir config apply` to hot-reload"),
            Err(e) => {
                println!("WARNING: {e}\n(the loader will fall back to DEFAULTS on this file)")
            }
        },
        Err(_) => println!("no file written"),
    }
    Ok(())
}

/// kubectl-apply for the memory (#52): validate, then SIGHUP every process
/// with real reload semantics. The MCP server and the gateway rebuild their
/// client from the re-read file and swap atomically; daemon/watch hold
/// deeper config snapshots and are listed as restart-to-apply.
fn config_apply() -> Result<()> {
    let p = config_target_path()?;
    match std::fs::read_to_string(&p) {
        Ok(content) => config_validate(&content)?,
        Err(_) => println!(
            "note: {} does not exist — defaults + env apply",
            p.display()
        ),
    }
    println!("config valid: {}", p.display());

    #[cfg(unix)]
    {
        let out = std::process::Command::new("pgrep")
            .args(["-f", "helixir-mcp|helixir gateway"])
            .output()?;
        let pids: Vec<i32> = String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .filter(|pid| *pid != std::process::id() as i32)
            .collect();
        if pids.is_empty() {
            println!("no running MCP/gateway processes found — nothing to signal");
        }
        for pid in pids {
            let ok = std::process::Command::new("kill")
                .args(["-HUP", &pid.to_string()])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            println!(
                "SIGHUP -> pid {pid}: {}",
                if ok {
                    "reloading (client rebuilt + swapped)"
                } else {
                    "FAILED"
                }
            );
        }
        for name in ["daemon", "watch"] {
            if let Some(state) = read_pid_state(name) {
                let pid = state.get("pid").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                if is_alive(pid) {
                    println!(
                        "{name} (pid {pid}): restart to apply — `helixir {}`",
                        if name == "daemon" {
                            "daemon stop && helixir daemon start"
                        } else {
                            "watch stop && helixir watch start"
                        }
                    );
                }
            }
        }
        println!("note: active FastThink sessions keep their pre-reload memory handle by design");
        println!(
            "note: processes running a binary OLDER than the hot-reload feature EXIT on SIGHUP\n      (no handler installed) — their supervisor/client restarts them with the new config"
        );
    }
    #[cfg(not(unix))]
    println!("hot-reload signaling is unix-only; restart processes to apply");
    Ok(())
}

fn cmd_name(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::Clotho { .. } => "clotho",
        Cmd::Lachesis { .. } => "lachesis",
        Cmd::Atropos { .. } => "atropos",
        Cmd::Pipeline { .. } => "pipeline",
        Cmd::Daemon { .. } => "daemon run",
        Cmd::Swarm { .. } => "swarm",
        Cmd::Heartbeat { .. } => "heartbeat",
        Cmd::Debt { .. } => "debt",
        Cmd::Watch { .. } => "watch",
        Cmd::Charter => "charter",
        Cmd::PruneAgent { .. } => "prune-agent",
        Cmd::Health { .. } => "health",
        _ => "command",
    }
}

/// Print the effective privilege tier and what it permits.
fn print_mode() -> Result<()> {
    // Layered config (toml + env), same as the gates — a raw env read here
    // showed "solo" while every gate honored the toml's Insights.
    let mode = helixir::core::config::HelixirConfig::from_env().mode;
    let on = |b: bool| if b { "ON" } else { "off" };
    println!("Privilege tier: {} (HELIXIR_MODE)", mode.label());
    println!(
        "  cross-user collective (link / contradict / collective reads): {}",
        on(mode.collective_enabled())
    );
    println!(
        "  generative insights (Clotho/Lachesis/Atropos, daemon):        {}",
        on(mode.insights_enabled())
    );
    if !mode.insights_enabled() {
        println!("\nRaise it: HELIXIR_MODE=collective|insights, or `helixir setup --mode <tier>`.");
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "helixir=info".into()),
        )
        .init();

    let cli = Cli::parse();

    // Daemon process management touches no DB — handle it before connecting, so
    // `stop`/`status` work even when HelixDB is down.
    if let Cmd::Daemon { cmd } = &cli.cmd {
        match cmd {
            DaemonCmd::Start {
                user,
                interval,
                threshold,
                max_seeds,
                max_hops,
                clotho_every,
                insight_every,
                merge_every,
                reconcile_every,
            } => {
                // The background daemon is generative — gate it on insights mode
                // before spawning (the child would otherwise fail in the dark).
                // Use the LAYERED config (helixir.toml + env), same as mode_gate —
                // a raw env read here ignored the toml and rejected valid setups.
                let mode = helixir::core::config::HelixirConfig::from_env().mode;
                if !mode.insights_enabled() {
                    anyhow::bail!(
                        "daemon needs mode=insights (current: {}); set it in ~/.helixir/helixir.toml or HELIXIR_MODE",
                        mode.label()
                    );
                }
                return daemon_start(
                    user,
                    *interval,
                    *threshold,
                    *max_seeds,
                    *max_hops,
                    [
                        ("--clotho-every", *clotho_every),
                        ("--insight-every", *insight_every),
                        ("--merge-every", *merge_every),
                        ("--reconcile-every", *reconcile_every),
                    ],
                );
            }
            DaemonCmd::Stop => return daemon_stop(),
            DaemonCmd::Status => return daemon_status(),
            DaemonCmd::Run { .. } => {} // needs the client — fall through
        }
    }
    if let Cmd::Config { cmd } = &cli.cmd {
        return match cmd {
            ConfigCmd::Get { raw } => config_get(*raw),
            ConfigCmd::Set { key, value } => config_set(key, value),
            ConfigCmd::Edit => config_edit(),
            ConfigCmd::Apply => config_apply(),
        };
    }
    if let Cmd::Watch { cmd } = &cli.cmd {
        match cmd {
            WatchCmd::Start { interval } => return watch_start(*interval),
            WatchCmd::Stop => return stop_process("watch"),
            WatchCmd::Status => {
                let Some(state) = read_pid_state("watch") else {
                    println!("watch: stopped (no pid file)");
                    return Ok(());
                };
                let pid = state.get("pid").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                println!(
                    "watch: {}  pid={pid}  journal={}",
                    if is_alive(pid) {
                        "running"
                    } else {
                        "STALE (process gone)"
                    },
                    helixir::agents::hygieia::journal_path().display()
                );
                return Ok(());
            }
            WatchCmd::Install => return watch_install(),
            WatchCmd::Uninstall => return watch_uninstall(),
            WatchCmd::Run { .. } => {} // needs the client — fall through
        }
    }
    if let Cmd::Health { tail } = &cli.cmd {
        return health_tail(*tail);
    }

    // Gateway: Run serves over HTTP (its own mcp-style client init); Start/Stop/
    // Status are process management (no DB) — all handled before the shared init.
    if let Cmd::Gateway { cmd } = &cli.cmd {
        return match cmd {
            GatewayCmd::Run { bind, require_auth } => {
                let config = helixir::core::config::HelixirConfig::from_env();
                let bind = bind.as_deref().unwrap_or(&config.gateway.default_bind);
                helixir::mcp::run_gateway_with_options(bind, *require_auth).await
            }
            GatewayCmd::Start { bind, require_auth } => {
                let config = helixir::core::config::HelixirConfig::from_env();
                let bind = bind.as_deref().unwrap_or(&config.gateway.default_bind);
                gateway_start(bind, *require_auth)
            }
            GatewayCmd::Stop => stop_process("gateway"),
            GatewayCmd::Status => gateway_status(),
        };
    }

    // `mode` just reports the effective tier — no DB needed.
    if matches!(&cli.cmd, Cmd::Mode) {
        return print_mode();
    }

    // `model` manages the local NLI model — no DB needed.
    if let Cmd::Model { sub } = &cli.cmd {
        return model_cmd(sub).await;
    }

    // Setup configures files + client configs; no DB connection needed.
    if let Cmd::Setup {
        non_interactive,
        dry_run,
        gateway,
        target,
        mode,
    } = &cli.cmd
    {
        return setup_run(
            !non_interactive,
            *dry_run,
            target.clone(),
            gateway.clone(),
            mode.clone(),
        )
        .await;
    }

    let client = HelixirClient::from_env().context("from_env (set HELIX_* env)")?;
    mode_gate(&cli.cmd, client.config().mode)?;
    if matches!(&cli.cmd, Cmd::Watch { .. }) {
        // The watchdog must survive a DEAD database — that is its job. A
        // failed initialize is Hygieia's first finding, not a fatal error.
        if let Err(e) = client.initialize().await {
            eprintln!("hygieia: initialize failed ({e}) — proceeding, the patient looks down");
        }
    } else {
        client.initialize().await.context("initialize")?;
    }

    match cli.cmd {
        Cmd::Config { .. } => unreachable!("handled before client construction"),
        Cmd::Charter => charter_review(&client).await?,
        Cmd::PruneAgent { agent_id, yes } => swarm_prune(&client, &agent_id, yes).await?,
        Cmd::Categories { limit } => categories(&client, limit).await?,
        Cmd::Clotho { cmd } => match cmd {
            ClothoCmd::Seed => clotho_seed(&client).await?,
            ClothoCmd::Tag {
                user,
                limit,
                threshold,
                top_k,
            } => clotho_tag(&client, &user, limit, threshold, top_k).await?,
            ClothoCmd::Grow {
                user,
                limit,
                threshold,
            } => clotho_grow(&client, &user, limit, threshold).await?,
        },
        Cmd::Lachesis { cmd } => match cmd {
            LachesisCmd::Pmi {
                cat_a,
                cat_b,
                universe,
            } => lachesis_pmi(&client, &cat_a, &cat_b, universe).await?,
            LachesisCmd::Route {
                seed,
                universe,
                max_hops,
            } => lachesis_route(&client, &seed, universe, max_hops).await?,
        },
        Cmd::Chain {
            user,
            topic,
            max_hops,
        } => chain(&client, &user, &topic, max_hops).await?,
        Cmd::Journal { tail } => journal_tail(tail)?,
        Cmd::Atropos {
            limit,
            max_seeds,
            max_hops,
        } => atropos_run(&client, limit, max_seeds, max_hops).await?,
        Cmd::Insights { tail } => insights_tail(tail)?,
        Cmd::Pipeline {
            user,
            threshold,
            max_seeds,
            max_hops,
        } => pipeline_run(&client, &user, threshold, max_seeds, max_hops).await?,
        Cmd::Debt {
            user,
            limit,
            reconcile,
        } => debt(&client, &user, limit, reconcile).await?,
        Cmd::Backfill { limit } => backfill(&client, limit).await?,
        Cmd::Merge { limit, threshold } => merge_run(&client, limit, threshold).await?,
        Cmd::Swarm { window } => swarm(&client, window).await?,
        Cmd::Heartbeat {
            agent,
            role,
            host,
            status,
        } => heartbeat(&client, &agent, &role, &host, &status).await?,
        Cmd::Daemon { cmd } => match cmd {
            DaemonCmd::Run {
                user,
                interval,
                once,
                threshold,
                max_seeds,
                max_hops,
                clotho_every,
                insight_every,
                merge_every,
                reconcile_every,
            } => {
                daemon_run(
                    &client,
                    user,
                    interval,
                    once,
                    threshold,
                    max_seeds,
                    max_hops,
                    [clotho_every, insight_every, merge_every, reconcile_every],
                )
                .await?
            }
            _ => unreachable!("daemon start/stop/status handled before client init"),
        },
        Cmd::Watch { cmd } => match cmd {
            WatchCmd::Run { once, interval } => watch_run(&client, once, interval).await?,
            _ => unreachable!("watch start/stop/status handled before client init"),
        },
        Cmd::Health { .. } => unreachable!("health handled before client init"),
        Cmd::Setup { .. } => unreachable!("setup handled before client init"),
        Cmd::Gateway { .. } => unreachable!("gateway handled before client init"),
        Cmd::Mode => unreachable!("mode handled before client init"),
        Cmd::Model { .. } => unreachable!("model handled before client init"),
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn daemon_run(
    client: &HelixirClient,
    user: String,
    interval: u64,
    once: bool,
    threshold: f64,
    max_seeds: usize,
    max_hops: usize,
    cadence: [Option<u64>; 4],
) -> Result<()> {
    // Per-stage cadence: CLI flag → else moira.daemon.*_every_passes (config).
    let d = &client.config().moira.daemon;
    let [clotho_every, insight_every, merge_every, reconcile_every] = cadence;
    let cfg = DaemonConfig {
        user: user.clone(),
        interval: Duration::from_secs(interval),
        once,
        host: machine_host(""),
        pass: PassConfig {
            grow_threshold: threshold,
            max_seeds,
            max_hops,
            ..PassConfig::default()
        },
        clotho_every: clotho_every.unwrap_or(d.clotho_every_passes),
        insight_every: insight_every.unwrap_or(d.insight_every_passes),
        merge_every: merge_every.unwrap_or(d.merge_every_passes),
        reconcile_every: reconcile_every.unwrap_or(d.reconcile_every_passes),
        stitch_every: d.stitch_every_passes,
        verify_every: d.verify_every_passes,
    };
    client
        .daemon()
        .run(cfg, |pass, run| {
            for ins in &run.insights {
                write_insight(ins);
            }
            println!(
                "[daemon] pass {pass} for '{user}': Clotho minted={} reused={}; Atropos {} insights",
                run.grow.minted,
                run.grow.reused_mint,
                run.insights.len()
            );
            journal(
                "daemon",
                "pass",
                &format!(
                    "user={user} pass={pass} minted={} insights={}",
                    run.grow.minted,
                    run.insights.len()
                ),
            );
        })
        .await?;
    Ok(())
}

async fn pipeline_run(
    client: &HelixirClient,
    user: &str,
    threshold: f64,
    max_seeds: usize,
    max_hops: usize,
) -> Result<()> {
    let cfg = PassConfig {
        grow_threshold: threshold,
        max_seeds,
        max_hops,
        ..PassConfig::default()
    };
    println!("Orchestrated pass for '{user}' (Clotho → Lachesis → Atropos)...");
    let run = client.orchestrator().full_pass(user, &cfg).await?;
    println!(
        "Clotho: matched={} minted={} reused={}",
        run.grow.tagged_by_match, run.grow.minted, run.grow.reused_mint
    );
    println!("Atropos: {} insights (journaled):", run.insights.len());
    for ins in &run.insights {
        write_insight(ins);
        println!(
            "  ★ value {:.2}  [{} hops, min PMI {:.2}]  {}",
            ins.value,
            ins.hops,
            ins.min_pmi,
            ins.category_path.join(" → ")
        );
    }
    journal(
        "orchestrator",
        "full_pass",
        &format!(
            "user={user} minted={} insights={}",
            run.grow.minted,
            run.insights.len()
        ),
    );
    Ok(())
}

async fn atropos_run(
    client: &HelixirClient,
    limit: i64,
    max_seeds: usize,
    max_hops: usize,
) -> Result<()> {
    let candidates = client.tooling().list_categories(limit).await?;
    let universe = resolve_universe(client, None).await?;
    let seeds: Vec<(String, String)> = candidates.iter().take(max_seeds).cloned().collect();
    println!(
        "Atropos curating from {} seeds over {} candidates (N={universe})...",
        seeds.len(),
        candidates.len()
    );
    let insights = client
        .atropos()
        .curate(&seeds, &candidates, universe, max_hops)
        .await?;

    println!("{} insights (journaled):", insights.len());
    for ins in &insights {
        write_insight(ins);
        println!(
            "  ★ value {:.2}  [{} hops, min PMI {:.2}]  {}",
            ins.value,
            ins.hops,
            ins.min_pmi,
            ins.category_path.join(" → ")
        );
        for w in ins.witnesses.iter().take(2) {
            println!("       · {} :: {}", w.link, w.snippet);
        }
    }
    journal(
        "atropos",
        "run",
        &format!("seeds={} insights={}", seeds.len(), insights.len()),
    );
    Ok(())
}

async fn swarm_prune(client: &HelixirClient, agent_id: &str, yes: bool) -> Result<()> {
    if !yes {
        println!(
            "Refusing to prune '{agent_id}' without --yes.\n\
             This deletes the presence row AND its AGENT_CREATED provenance \
             edges — meant for true junk (test agents, renamed identities). \
             A merely-stale agent is already flagged in swarm_status."
        );
        return Ok(());
    }
    client
        .db()
        .execute_query::<serde_json::Value, _>(
            "dropPresenceByAgentId",
            &serde_json::json!({"agent_id": agent_id}),
        )
        .await?;
    println!("Pruned presence row for '{agent_id}'.");
    Ok(())
}

async fn charter_review(client: &HelixirClient) -> Result<()> {
    let tooling = client.tooling();
    let threshold = client.config().write.rule_propose_after;
    let rules = tooling.learned_charter_rules().await;
    let precedents = tooling.charter_precedent_counts().await;

    println!("Memory charter — constitution + learned rules");
    println!("  constitution: helixir/memory-charter.md (override: ~/.helixir/memory-charter.md)");
    println!("  full text with learned rules: MCP resource memory://rules\n");

    println!("Adopted rules: {}", rules.len());
    for r in &rules {
        println!("  - {}", r.chars().take(120).collect::<String>());
    }

    println!("\nPrecedents by shape (proposal after {threshold} identical verdicts):");
    if precedents.is_empty() {
        println!("  (none yet — precedents accumulate from resolve_contradiction verdicts)");
    }
    for (shape, n) in &precedents {
        let adopted = rules.iter().any(|r| r.contains(&format!("[{shape}]")));
        let status = if adopted {
            "rule adopted".to_string()
        } else if *n >= threshold {
            "proposal ripe — next identical verdict returns it".to_string()
        } else {
            format!("{} more to a proposal", threshold - n)
        };
        println!("  {shape}: {n} episode(s) — {status}");
    }
    Ok(())
}

async fn categories(client: &HelixirClient, limit: i64) -> Result<()> {
    let cats = client.tooling().list_categories(limit).await?;
    let mut rows = Vec::with_capacity(cats.len());
    for (id, name) in cats {
        let n = client.tooling().category_member_ids(&id).await?.len();
        rows.push((n, name, id));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    println!("{} categories (by member count):", rows.len());
    for (n, name, id) in &rows {
        println!("  {n:>6}  {name}   [{id}]");
    }
    Ok(())
}

async fn clotho_seed(client: &HelixirClient) -> Result<()> {
    let n = client.clotho().seed_dictionary().await?;
    println!("seeded {n} categories");
    journal("clotho", "seed", &format!("ensured {n} categories"));
    Ok(())
}

async fn clotho_tag(
    client: &HelixirClient,
    user: &str,
    limit: i64,
    threshold: f64,
    top_k: i64,
) -> Result<()> {
    let mems = client.tooling().list_user_memories(user, limit).await?;
    println!(
        "Clotho tagging {} memories for '{user}' (bar {threshold})...",
        mems.len()
    );
    let (mut tags, mut escalations, mut tagged_mems) = (0usize, 0usize, 0usize);
    for (id, content) in &mems {
        let outcome = client
            .clotho()
            .auto_tag(id, content, top_k, threshold)
            .await?;
        if !outcome.tagged.is_empty() {
            tagged_mems += 1;
            tags += outcome.tagged.len();
            let names: Vec<String> = outcome
                .tagged
                .iter()
                .map(|h| format!("{}={:.2}", h.name, h.score))
                .collect();
            println!("  [{id}] {names:?}");
        }
        if outcome.escalation.is_some() {
            escalations += 1;
        }
    }
    println!(
        "done: {tagged_mems}/{} memories tagged, {tags} tags, {escalations} escalations",
        mems.len()
    );
    journal(
        "clotho",
        "tag",
        &format!(
            "user={user} scanned={} tagged={tagged_mems} tags={tags} escalations={escalations}",
            mems.len()
        ),
    );
    Ok(())
}

async fn clotho_grow(client: &HelixirClient, user: &str, limit: i64, threshold: f64) -> Result<()> {
    let mems = client.tooling().list_user_memories(user, limit).await?;
    println!(
        "Clotho grow-pass over {} memories for '{user}' (bar {threshold}); minting on miss...",
        mems.len()
    );
    let s = client.clotho().grow_pass(&mems, threshold).await?;
    println!(
        "done: scanned={} matched={} minted={} reused={} failed={}",
        s.scanned, s.tagged_by_match, s.minted, s.reused_mint, s.failed
    );
    journal(
        "clotho",
        "grow",
        &format!(
            "user={user} scanned={} matched={} minted={} reused={} failed={}",
            s.scanned, s.tagged_by_match, s.minted, s.reused_mint, s.failed
        ),
    );
    Ok(())
}

async fn lachesis_pmi(
    client: &HelixirClient,
    cat_a: &str,
    cat_b: &str,
    universe: Option<usize>,
) -> Result<()> {
    let universe = resolve_universe(client, universe).await?;
    let p = client.lachesis().subset_pmi(cat_a, cat_b, universe).await?;
    println!("PMI({cat_a}, {cat_b}) over N={universe} = {p:.4}");
    if p.is_finite() {
        println!(
            "  → {}",
            if p >= 0.5 {
                "above chance — a real, surprising overlap"
            } else {
                "at/below chance — not a meaningful link"
            }
        );
    } else {
        println!("  → the two subsets never co-occur");
    }
    Ok(())
}

async fn lachesis_route(
    client: &HelixirClient,
    seed: &str,
    universe: Option<usize>,
    max_hops: usize,
) -> Result<()> {
    let universe = resolve_universe(client, universe).await?;
    let candidates = client.tooling().list_categories(500).await?;
    let hypo = client
        .lachesis()
        .route_subsets(seed, &candidates, universe, max_hops)
        .await?;
    match hypo {
        Some(h) => {
            println!(
                "subset thread ({} hops, min PMI {:.3}, requires verification):",
                h.hops, h.min_pmi
            );
            for (i, s) in h.steps.iter().enumerate() {
                if i == 0 {
                    println!("  {}", s.category_name);
                } else {
                    println!("  └─[PMI {:.2}]→ {}", s.pmi_from_prev, s.category_name);
                    for w in &s.witnesses {
                        println!("        · witness [{}] {}", w.memory_id, w.snippet);
                    }
                }
            }
            journal(
                "lachesis",
                "route",
                &format!(
                    "seed={seed} hops={} min_pmi={:.3} chain={}",
                    h.hops,
                    h.min_pmi,
                    h.steps
                        .iter()
                        .map(|s| s.category_name.as_str())
                        .collect::<Vec<_>>()
                        .join("→")
                ),
            );
        }
        None => {
            println!("no qualifying subset thread from [{seed}] (no above-chance neighbour)");
            journal("lachesis", "route", &format!("seed={seed} result=none"));
        }
    }
    Ok(())
}

async fn chain(client: &HelixirClient, user: &str, topic: &str, max_hops: usize) -> Result<()> {
    match client.longest_chain(topic, user, max_hops).await? {
        Some(n) => {
            println!(
                "longest chain: {} hops, confidence {:.4}",
                n.hops, n.confidence
            );
            for (i, s) in n.steps.iter().enumerate() {
                let edge = s
                    .edge_type
                    .as_deref()
                    .map(|t| format!(" ─[{t} {:.2}]→", s.edge_weight))
                    .unwrap_or_default();
                let snippet: String = s.content.chars().take(80).collect();
                println!("  {i}.{edge} {snippet}");
            }
        }
        None => println!("no reasoning chain found for '{topic}'"),
    }
    Ok(())
}

/// PMI universe N: explicit, else the total memory count.
async fn resolve_universe(client: &HelixirClient, universe: Option<usize>) -> Result<usize> {
    match universe {
        Some(u) => Ok(u),
        None => Ok(client.tooling().total_memory_count(1_000_000).await?.max(1)),
    }
}

// --- setup wizard: configure + wire the MCP server into agent clients ---

struct SetupConfig {
    host: String,
    port: String,
    instance: String,
    llm_provider: String,
    llm_model: String,
    llm_key: String,
    emb_provider: String,
    emb_model: String,
    emb_url: String,
    mcp_bin: String,
    /// Privilege tier written as HELIXIR_MODE (default solo).
    mode: String,
}

fn default_mcp_bin() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("helixir-mcp")))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "helixir-mcp".to_string())
}

fn gather_config(interactive: bool, discovered: Option<(String, u16)>) -> Result<SetupConfig> {
    let e = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    let mut c = SetupConfig {
        host: e("HELIX_HOST", "localhost"),
        port: e("HELIX_PORT", "6970"),
        instance: e("HELIX_INSTANCE", "bench"),
        llm_provider: e("HELIX_LLM_PROVIDER", "ollama"),
        llm_model: e("HELIX_LLM_MODEL", "llama3.1:8b"),
        llm_key: e("HELIX_LLM_API_KEY", ""),
        emb_provider: e("HELIX_EMBEDDING_PROVIDER", "ollama"),
        emb_model: e("HELIX_EMBEDDING_MODEL", "nomic-embed-text"),
        emb_url: e("HELIX_EMBEDDING_URL", "http://localhost:11434"),
        mcp_bin: default_mcp_bin(),
        mode: e("HELIXIR_MODE", "solo"),
    };
    // A discovered backend pre-fills host/port — but only where the user has not
    // explicitly pinned them via env, so a scripted run with HELIX_* still wins.
    if let Some((h, p)) = discovered {
        if std::env::var("HELIX_HOST").is_err() {
            c.host = h;
        }
        if std::env::var("HELIX_PORT").is_err() {
            c.port = p.to_string();
        }
    }
    if interactive {
        let ask = |prompt: &str, def: &str| -> Result<String> {
            Ok(Input::<String>::new()
                .with_prompt(prompt)
                .default(def.to_string())
                .allow_empty(true)
                .interact_text()?)
        };
        c.host = ask("HelixDB host", &c.host)?;
        c.port = ask("HelixDB port", &c.port)?;
        c.instance = ask("HelixDB instance", &c.instance)?;
        c.llm_provider = ask("LLM provider (cerebras / ollama)", &c.llm_provider)?;
        c.llm_model = ask("LLM model", &c.llm_model)?;
        c.llm_key = ask("LLM API key (blank for local)", &c.llm_key)?;
        c.emb_model = ask("Embedding model", &c.emb_model)?;
        c.emb_url = ask("Embedding URL", &c.emb_url)?;
        c.mcp_bin = ask("Path to the helixir-mcp binary", &c.mcp_bin)?;
    }
    Ok(c)
}

fn mcp_entry(c: &SetupConfig) -> serde_json::Value {
    serde_json::json!({
        "command": c.mcp_bin,
        "args": [],
        "env": {
            "HELIXIR_SELF_SEED": "1",
            "HELIX_HOST": c.host,
            "HELIX_PORT": c.port,
            "HELIX_INSTANCE": c.instance,
            "HELIX_LLM_PROVIDER": c.llm_provider,
            "HELIX_LLM_MODEL": c.llm_model,
            "HELIX_LLM_API_KEY": c.llm_key,
            "HELIX_EMBEDDING_PROVIDER": c.emb_provider,
            "HELIX_EMBEDDING_MODEL": c.emb_model,
            "HELIX_EMBEDDING_URL": c.emb_url,
            "HELIXIR_RETRIEVAL_PROFILE": "algo_opt",
            "HELIXIR_MODE": c.mode,
        }
    })
}

/// Normalize a gateway arg (URL or `host:port`) to a full streamable-http URL.
fn normalize_gateway_url(raw: &str) -> String {
    let s = raw.trim();
    let with_scheme = if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("http://{s}")
    };
    if with_scheme.trim_end_matches('/').ends_with("/mcp") {
        with_scheme
    } else {
        format!("{}/mcp", with_scheme.trim_end_matches('/'))
    }
}

/// Client entry for a remote gateway: HTTP transport, no command, no env — the
/// gateway holds all the HELIX_* config.
fn mcp_entry_gateway(url: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "http",
        "url": url,
    })
}

fn client_targets() -> Vec<(String, PathBuf)> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let desktop = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Claude/claude_desktop_config.json")
    } else {
        home.join(".config/Claude/claude_desktop_config.json")
    };
    vec![
        ("Claude Code".to_string(), home.join(".claude.json")),
        ("Claude Desktop".to_string(), desktop),
        ("Cursor".to_string(), home.join(".cursor/mcp.json")),
        ("Gemini CLI".to_string(), home.join(".gemini/settings.json")),
    ]
}

/// Merge the `helixir-local` MCP entry into a client's config JSON (creating
/// `mcpServers` if absent), backing the file up first. Non-destructive: other
/// servers and keys are preserved.
fn wire_client(name: &str, path: &Path, entry: &serde_json::Value, dry_run: bool) -> Result<()> {
    let mut root: serde_json::Value = if path.exists() {
        serde_json::from_str(&std::fs::read_to_string(path)?)
            .unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    if !root.is_object() {
        root = serde_json::json!({});
    }
    let servers = root
        .as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    if !servers.is_object() {
        *servers = serde_json::json!({});
    }
    servers
        .as_object_mut()
        .unwrap()
        .insert("helixir-local".to_string(), entry.clone());

    if dry_run {
        println!(
            "  [dry-run] {name}: would set helixir-local in {}",
            path.display()
        );
        return Ok(());
    }
    if path.exists() {
        std::fs::copy(path, PathBuf::from(format!("{}.bak", path.display()))).ok();
    } else if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(path, serde_json::to_string_pretty(&root)?)?;
    println!(
        "  ✓ {name}: wired helixir-local → {} (backup .bak)",
        path.display()
    );
    Ok(())
}

/// Probe one `host:port` for a live HelixDB via the real client health check,
/// bounded so a filtered port cannot hang the wizard.
async fn probe_backend(host: &str, port: u16) -> bool {
    let Ok(client) = HelixClient::new(host, port) else {
        return false;
    };
    let probe = async {
        let _ = client.connect().await;
        client.health_check().await
    };
    matches!(
        tokio::time::timeout(Duration::from_millis(1500), probe).await,
        Ok(Ok(()))
    )
}

/// Probe the local machine for a live HelixDB so a second client connects to the
/// existing backend instead of standing up a duplicate (the singleton rule). The
/// env-pinned port is tried first, then the common Helix ports.
async fn discover_backends() -> Vec<(String, u16)> {
    let host = std::env::var("HELIX_HOST").unwrap_or_else(|_| "localhost".to_string());
    let env_port: Option<u16> = std::env::var("HELIX_PORT")
        .ok()
        .and_then(|p| p.parse().ok());
    let mut ports: Vec<u16> = Vec::new();
    for p in env_port.into_iter().chain([6970u16, 6969]) {
        if !ports.contains(&p) {
            ports.push(p);
        }
    }
    let mut live = Vec::new();
    for port in ports {
        if probe_backend(&host, port).await {
            live.push((host.clone(), port));
        }
    }
    live
}

/// The honest "it works" gate: prove the configured backend actually answers a
/// health check before we tell the user their clients are wired.
async fn verify_backend(cfg: &SetupConfig) -> Result<()> {
    let port: u16 = cfg.port.parse().context("HelixDB port must be a number")?;
    let client = HelixClient::new(&cfg.host, port).map_err(|e| anyhow::anyhow!("{e}"))?;
    let probe = async {
        let _ = client.connect().await;
        client
            .health_check()
            .await
            .map_err(|e| anyhow::anyhow!("health check failed: {e}"))
    };
    match tokio::time::timeout(Duration::from_secs(5), probe).await {
        Ok(inner) => inner,
        Err(_) => anyhow::bail!(
            "timed out after 5s — no HelixDB answering at {}:{}",
            cfg.host,
            port
        ),
    }
}

/// Best-effort primary LAN IP: open a UDP socket "toward" a public address and
/// read which local interface the OS would route through. Sends no packet.
fn lan_ip() -> Option<std::net::IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    let ip = sock.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// Interactive privilege-tier picker for `helixir setup` when no tier was
/// stated via `--mode` or HELIXIR_MODE. Collective is the recommended default
/// (index 0): a person running the wizard is consciously joining the shared
/// memory, which is the point of the tool. Solo and Insights stay one keystroke
/// away, and the silent library default (HelixirConfig::new) remains Solo.
fn prompt_mode_recommendation() -> Result<MemoryMode> {
    let options = [
        "collective — shared memory across your agents (recommended)",
        "solo — private, single user, no cross-user behaviour",
        "insights — collective + the generative Moirai (advanced)",
    ];
    let idx = Select::new()
        .with_prompt("Privilege tier")
        .default(0)
        .items(&options)
        .interact()?;
    Ok([
        MemoryMode::Collective,
        MemoryMode::Solo,
        MemoryMode::Insights,
    ][idx])
}

/// During setup, for the collective/insights tiers, surface and optionally fetch
/// the local NLI model (the paraphrase-merge judge). Solo skips it entirely.
#[cfg(feature = "nli")]
async fn maybe_setup_nli_model(mode: MemoryMode, interactive: bool, dry_run: bool) -> Result<()> {
    use helixir::llm::nli;
    if !mode.collective_enabled() {
        return Ok(());
    }
    let s = nli::status();
    println!(
        "Paraphrase merging ({}) uses a local NLI model — variant {} for {}.",
        mode.label(),
        s.variant_for_host,
        s.host
    );
    if s.installed {
        println!(
            "  ✓ already installed ({:.0} MB at {}).\n",
            s.onnx_bytes as f64 / 1e6,
            s.dir.display()
        );
        return Ok(());
    }
    if dry_run {
        println!("  (dry-run: would download ~90 MB)\n");
        return Ok(());
    }
    let go = interactive
        && Confirm::new()
            .with_prompt("Download the NLI model now (~90 MB)?")
            .default(true)
            .interact()?;
    if go {
        let bytes = nli::download(false).await?;
        match nli::NliJudge::load(&nli::NliJudge::default_dir()) {
            Ok(_) => println!(
                "  ✓ fetched {:.0} MB — NLI model ready.\n",
                bytes as f64 / 1e6
            ),
            Err(e) => println!("  ⚠ downloaded but failed to load: {e}\n"),
        }
    } else {
        println!("  skipped — run `helixir model download` when ready.\n");
    }
    Ok(())
}

/// No-op when built without the `nli` feature: paraphrase merging is unavailable,
/// so setup just proceeds without offering the model.
#[cfg(not(feature = "nli"))]
async fn maybe_setup_nli_model(
    _mode: MemoryMode,
    _interactive: bool,
    _dry_run: bool,
) -> Result<()> {
    Ok(())
}

async fn setup_run(
    interactive: bool,
    dry_run: bool,
    target: Option<String>,
    gateway: Option<String>,
    mode: Option<String>,
) -> Result<()> {
    println!("Helixir setup — configure + wire its MCP server into your agent clients\n");
    // Effective tier resolution. Explicit choice always wins (`--mode`, then
    // HELIXIR_MODE env) — we never override what the operator stated, including
    // an explicit `solo`. Only when nothing is stated does setup *recommend*:
    // a human running the wizard is consciously joining, so the collective (the
    // whole point of the tool) is the recommended pick. The silent library
    // default stays Solo (HelixirConfig::new) — embedded/non-onboarded callers
    // never get escalated without a person choosing it here.
    let env_mode = std::env::var("HELIXIR_MODE").unwrap_or_default();
    let effective_mode = match &mode {
        Some(m) => MemoryMode::parse(m),
        None if !env_mode.is_empty() => MemoryMode::parse(&env_mode),
        None if interactive => prompt_mode_recommendation()?,
        None => MemoryMode::Collective, // non-interactive setup → the recommendation
    };
    let mode_label = effective_mode.label();
    println!("Privilege tier: {mode_label} (HELIXIR_MODE).\n");

    // Collective/insights use a local NLI model for contradiction-safe paraphrase
    // merging — offer to fetch it now (solo never needs it).
    maybe_setup_nli_model(effective_mode, interactive, dry_run).await?;

    // Gateway mode short-circuits DB discovery: clients talk to the per-host
    // gateway over HTTP, which holds the HELIX_* config — they carry none.
    if let Some(gw) = gateway {
        let url = normalize_gateway_url(&gw);
        println!("Gateway mode — wiring clients to {url}");
        println!("  HTTP transport: clients carry no HELIX_* env; the gateway holds the config.");
        println!("  The privilege tier lives on the GATEWAY process — start it with");
        println!("  `HELIXIR_MODE={mode_label} helixir gateway start`, not on the client.\n");
        let entry = mcp_entry_gateway(&url);
        return wire_entry_to_clients(
            entry,
            target,
            interactive,
            dry_run,
            &format!("gateway {url}"),
        );
    }

    // 1. Discover — a HelixDB is a singleton; find an existing one so we connect
    //    rather than provision a second store nobody shares.
    println!("Looking for a live HelixDB on this machine…");
    let found = discover_backends().await;
    match found.first() {
        // Informational — the actual target is decided by config (env/prompt) and
        // shown by the verify line below; if you see a live one here but verify
        // points elsewhere, your HELIX_* env is pinned to a different port.
        Some((h, p)) => println!("  ✓ a live HelixDB is answering at {h}:{p}.\n"),
        None => {
            println!("  · none found on the usual ports.");
            println!("    → join an existing collective: set the host/port below to a reachable");
            println!("      HelixDB (e.g. another machine that ran setup → its LAN address).");
            println!("    → or deploy one here: `helix push` in a HelixDB project, then re-run.\n");
        }
    }

    let mut cfg = gather_config(interactive && target.is_none(), found.into_iter().next())?;
    cfg.mode = mode_label.to_string();

    // 2. Verify — prove the backend answers before claiming success. On failure,
    //    let the user wire anyway (interactive) or abort with the error.
    print!("Verifying {}:{} … ", cfg.host, cfg.port);
    std::io::stdout().flush().ok();
    match verify_backend(&cfg).await {
        Ok(()) => println!("ok — HelixDB is reachable.\n"),
        Err(e) => {
            println!("FAILED\n  {e}\n");
            if interactive
                && !Confirm::new()
                    .with_prompt("Backend did not verify — wire the client(s) anyway?")
                    .default(false)
                    .interact()?
            {
                println!("Aborted — fix the host/port or deploy HelixDB, then re-run.");
                return Ok(());
            }
        }
    }

    // 3. Multi-host — if this machine hosts the (local) DB, surface the LAN
    //    address other hosts point their client at to join the same collective.
    //    That is the rendezvous (#39) in practice: one shared DB, many hosts.
    let host_is_local = matches!(
        cfg.host.as_str(),
        "localhost" | "127.0.0.1" | "0.0.0.0" | "::1"
    );
    if host_is_local {
        match lan_ip() {
            Some(ip) => {
                println!("This machine's LAN address: {ip}:{}", cfg.port);
                println!("  Other hosts join the same collective by setting their client's");
                println!("  HELIX_HOST={ip} (full network trust assumed — no auth token yet).\n");
            }
            None => println!("(No LAN address found — offline, or no network interface.)\n"),
        }
    }

    let entry = mcp_entry(&cfg);
    let source = format!("helixir-mcp at {}", cfg.mcp_bin);
    wire_entry_to_clients(entry, target, interactive, dry_run, &source)
}

/// Wire a prepared MCP entry into clients: an explicit `--target` file, else the
/// detected clients (multi-select when interactive). `source` labels what is
/// being wired (a stdio binary path or a gateway URL) in the output.
fn wire_entry_to_clients(
    entry: serde_json::Value,
    target: Option<String>,
    interactive: bool,
    dry_run: bool,
    source: &str,
) -> Result<()> {
    if let Some(t) = target {
        let path = PathBuf::from(&t);
        println!("Wiring helixir-local ({source}):");
        wire_client("target", &path, &entry, dry_run)?;
        println!(
            "{}",
            if dry_run {
                "\n(dry-run — nothing was written.)"
            } else {
                "\nDone."
            }
        );
        return Ok(());
    }

    let targets = client_targets();
    let selected: Vec<(String, PathBuf)> = if interactive {
        let labels: Vec<String> = targets
            .iter()
            .map(|(n, p)| {
                format!(
                    "{n}  [{}]{}",
                    p.display(),
                    if p.exists() { "" } else { " (new)" }
                )
            })
            .collect();
        let picks = MultiSelect::new()
            .with_prompt("Wire which clients? (space to toggle, enter to confirm)")
            .items(&labels)
            .interact()?;
        picks.into_iter().map(|i| targets[i].clone()).collect()
    } else {
        targets
    };

    if selected.is_empty() {
        println!("No clients selected — nothing to do.");
        return Ok(());
    }
    if interactive
        && !dry_run
        && !Confirm::new()
            .with_prompt("Write the helixir-local MCP entry to the selected clients?")
            .default(true)
            .interact()?
    {
        println!("Aborted — no changes made.");
        return Ok(());
    }

    println!("\nWiring helixir-local ({source}):");
    for (name, path) in &selected {
        if let Err(e) = wire_client(name, path, &entry, dry_run) {
            println!("  ✗ {name}: {e}");
        }
    }
    if dry_run {
        println!("\n(dry-run — nothing was written.)");
    } else {
        println!("\nDone. Restart the client(s) to pick up the helixir-local MCP server.");
    }
    Ok(())
}

// --- contradiction debt (#45): the Cutter's hygiene dashboard ---

#[cfg(feature = "nli")]
async fn model_cmd(sub: &ModelCmd) -> Result<()> {
    use helixir::llm::nli;
    match sub {
        ModelCmd::Which => {
            println!("host:                  {}", nli::host_label());
            println!("variant for this host: {}", nli::pick_onnx_variant());
            Ok(())
        }
        ModelCmd::Status => {
            let s = nli::status();
            println!("NLI model — host {}", s.host);
            println!("  dir:              {}", s.dir.display());
            println!("  installed:        {}", s.installed);
            if s.installed {
                println!("  model.onnx:       {:.1} MB", s.onnx_bytes as f64 / 1e6);
            }
            println!("  variant for host: {}", s.variant_for_host);
            if !s.installed {
                println!("\nRun `helixir model download` to fetch it (~90 MB).");
            }
            Ok(())
        }
        ModelCmd::Download { force } => {
            println!(
                "Downloading NLI model for {} (variant: {}) …",
                nli::host_label(),
                nli::pick_onnx_variant()
            );
            let bytes = nli::download(*force).await?;
            println!(
                "Fetched {:.1} MB into {}.\n",
                bytes as f64 / 1e6,
                nli::NliJudge::default_dir().display()
            );
            // Readiness immediately after install (agreed flow).
            nli_check()
        }
        ModelCmd::Check => nli_check(),
    }
}

#[cfg(not(feature = "nli"))]
async fn model_cmd(_sub: &ModelCmd) -> Result<()> {
    anyhow::bail!(
        "this build was compiled without the `nli` feature — rebuild with `--features nli` to use the model/NLI commands"
    )
}

#[cfg(feature = "nli")]
fn nli_check() -> Result<()> {
    use helixir::llm::nli::{NliJudge, NliLabel};

    let dir = NliJudge::default_dir();
    println!("Local NLI judge — liveness + readiness check");
    println!("Loading from {} …\n", dir.display());
    let mut judge = NliJudge::load(&dir).context(
        "load NLI model (collective/insights setup downloads it to ~/.helixir/models/nli)",
    )?;
    // Introspected, not assumed — this is what bit us before.
    println!("  model inputs : {:?}", judge.input_names());
    println!("  model outputs: {:?}\n", judge.output_names());

    let cases: &[(&str, &str)] = &[
        (
            "I prefer the dark theme in every editor.",
            "I prefer the light theme in every editor.",
        ),
        ("I love pizza.", "Pizza is my favourite food."),
        (
            "The deploy region is eu-west-3.",
            "The on-call rotation is weekly.",
        ),
    ];
    for (a, b) in cases {
        let (lab, sc) = judge.classify(a, b)?;
        let same = judge.is_same_fact(a, b)?;
        println!(
            "  [{:>13}]  same_fact={:<5}  c={:.2} e={:.2} n={:.2}",
            lab.as_str(),
            same,
            sc[0],
            sc[1],
            sc[2]
        );
        println!("      A: {a}");
        println!("      B: {b}");
    }

    // The two safety-critical invariants.
    let opposite_is_contra = judge.classify(cases[0].0, cases[0].1)?.0 == NliLabel::Contradiction;
    let opposite_not_merged = !judge.is_same_fact(cases[0].0, cases[0].1)?;
    let paraphrase_is_same = judge.is_same_fact(cases[1].0, cases[1].1)?;

    println!();
    println!(
        "  CRITICAL  opposite preference → contradiction : {}",
        if opposite_is_contra { "PASS" } else { "FAIL" }
    );
    println!(
        "  CRITICAL  opposite preference NOT merged      : {}",
        if opposite_not_merged { "PASS" } else { "FAIL" }
    );
    println!(
        "  CRITICAL  paraphrase → same fact              : {}",
        if paraphrase_is_same { "PASS" } else { "FAIL" }
    );

    if opposite_is_contra && opposite_not_merged && paraphrase_is_same {
        println!("\n✓ NLI judge READY — contradiction-safe, paraphrase-aware.");
        Ok(())
    } else {
        anyhow::bail!("NLI readiness check FAILED — model would be unsafe for paraphrase merges");
    }
}

#[cfg(feature = "nli")]
async fn merge_run(client: &HelixirClient, limit: i64, threshold: f64) -> Result<()> {
    use helixir::agents::atropos::Atropos;
    println!("Paraphrase backstop (#43/#55) — collective scan (cosine ≥ {threshold}) …");
    let atropos = Atropos::new(client.tooling());
    let s = atropos.merge_paraphrases(limit, threshold).await?;
    println!(
        "  scanned {} memories, {} candidate pairs above threshold",
        s.scanned, s.candidates
    );
    println!(
        "  merged {} fingerprint group(s) — {} node(s) re-stamped",
        s.merged_groups, s.nodes_restamped
    );
    println!(
        "  contradictions blocked from merging: {}",
        s.contradictions_blocked
    );
    Ok(())
}

#[cfg(not(feature = "nli"))]
async fn merge_run(_client: &HelixirClient, _limit: i64, _threshold: f64) -> Result<()> {
    anyhow::bail!(
        "this build was compiled without the `nli` feature — paraphrase merge is unavailable; rebuild with `--features nli`"
    )
}

async fn backfill(client: &HelixirClient, limit: i64) -> Result<()> {
    println!("Backfilling content_key fingerprints (#43 migration)…");
    let (scanned, updated) = client
        .tooling()
        .backfill_content_keys(limit)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!(
        "Scanned {scanned} memories — stamped {updated} new fingerprints (the rest were already keyed)."
    );
    Ok(())
}

async fn debt(client: &HelixirClient, user: &str, limit: i64, reconcile: bool) -> Result<()> {
    use helixir::agents::atropos::reconcile::{DisputeKind, classify};

    if reconcile {
        let s = client
            .atropos()
            .reconcile(user, limit)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        println!(
            "Reconciled '{user}': scanned {}, drained {} preference + {} superseded, {} live kept, {} surfaced to owners",
            s.scanned, s.drained_preference, s.drained_superseded, s.kept_live, s.notified
        );
        journal(
            "atropos",
            "reconcile",
            &format!(
                "user={user} drained={} kept={}",
                s.drained_preference + s.drained_superseded,
                s.kept_live
            ),
        );
        return Ok(());
    }

    let open = client
        .tooling()
        .gather_open_contradictions(user, limit)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if open.is_empty() {
        println!("No open contradiction debt for '{user}'.");
        return Ok(());
    }
    let (mut pref, mut live) = (0u32, 0u32);
    println!(
        "Open contradiction debt for '{user}' — {} dispute(s):\n",
        open.len()
    );
    for oc in &open {
        let tag = match classify(&oc.resolution_strategy) {
            DisputeKind::Preference => {
                pref += 1;
                "preference"
            }
            DisputeKind::Factual => {
                live += 1;
                "factual"
            }
        };
        println!(
            "  {} ⇄ {}  [{tag}]  {}",
            trunc(&oc.from_id, 16),
            trunc(&oc.to_id, 16),
            oc.resolution_strategy
        );
    }
    println!(
        "\n  {pref} preference (drainable as coexist) · {live} factual (live — need an owner)"
    );
    println!("  Run with --reconcile to retire the drainable ones.");
    Ok(())
}

// --- swarm rendezvous (#39): presence in the shared graph ---

/// Resolve a host label: explicit arg wins, else env hints, else "unknown".
fn machine_host(explicit: &str) -> String {
    if !explicit.is_empty() {
        return explicit.to_string();
    }
    std::env::var("HELIXIR_HOST_LABEL")
        .or_else(|_| std::env::var("HOSTNAME"))
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "unknown".to_string())
}

fn human_age(secs: i64) -> String {
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!(
            "{}…",
            s.chars().take(n.saturating_sub(1)).collect::<String>()
        )
    }
}

async fn heartbeat(
    client: &HelixirClient,
    agent: &str,
    role: &str,
    host: &str,
    status: &str,
) -> Result<()> {
    let host = machine_host(host);
    client
        .tooling()
        .register_or_heartbeat(agent, role, &host, status)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("✓ heartbeat: {agent} ({role}) on {host} — {status}");
    journal("swarm", "heartbeat", &format!("{agent}@{host}:{status}"));
    Ok(())
}

async fn swarm(client: &HelixirClient, window: Option<u64>) -> Result<()> {
    let window = window.unwrap_or(client.config().swarm.active_window_secs);
    let now = chrono::Utc::now();
    let mut roster = client
        .tooling()
        .list_swarm()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if roster.is_empty() {
        println!("No agents registered in the collective yet.");
        println!("(Run `helixir heartbeat --agent <id>` or start the daemon to announce one.)");
        return Ok(());
    }
    // Freshest first; never-seen sink to the bottom.
    roster.sort_by_key(|a| a.age_seconds(now).unwrap_or(i64::MAX));

    let win = window as i64;
    let active = roster.iter().filter(|a| a.is_active(now, win)).count();
    println!(
        "Swarm roster — {} agent(s), {active} active (heartbeat ≤{window}s)\n",
        roster.len()
    );
    println!(
        "     {:<22} {:<11} {:<16} {:<7} {}",
        "agent", "role", "host", "age", "status"
    );
    for a in &roster {
        let dot = if a.is_active(now, win) { "●" } else { "·" };
        let age = match a.age_seconds(now) {
            Some(s) if s >= 0 => human_age(s),
            _ => "never".to_string(),
        };
        println!(
            "  {dot}  {:<22} {:<11} {:<16} {:<7} {}",
            trunc(&a.agent_id, 22),
            trunc(&a.role, 11),
            trunc(&a.host, 16),
            age,
            a.status
        );
    }
    Ok(())
}

// --- daemon background lifecycle (PID file in ~/.helixir) ---

fn helixir_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    let dir = PathBuf::from(home).join(".helixir");
    std::fs::create_dir_all(&dir).ok();
    Ok(dir)
}

fn pid_file(name: &str) -> Result<PathBuf> {
    Ok(helixir_dir()?.join(format!("{name}.pid")))
}

fn read_pid_state(name: &str) -> Option<serde_json::Value> {
    let body = std::fs::read_to_string(pid_file(name).ok()?).ok()?;
    serde_json::from_str(&body).ok()
}

/// Signal 0 probes a pid's existence without delivering anything.
#[cfg(unix)]
fn is_alive(pid: i32) -> bool {
    pid > 0 && unsafe { libc::kill(pid, 0) == 0 }
}

/// Windows has no signal 0; the detached-process machinery is unix-only, so
/// any recorded pid is treated as gone (stale state files self-clean).
#[cfg(not(unix))]
fn is_alive(_pid: i32) -> bool {
    false
}

/// Spawn `helixir <args>` as a detached background process (setsid), logging to
/// `~/.helixir/<name>.log` and recording a `<name>.pid` state file. Shared by
/// the daemon (#43) and the gateway (#42). Returns the child pid.
#[cfg(unix)]
fn spawn_detached(name: &str, args: &[&str], extra: serde_json::Value) -> Result<(u32, PathBuf)> {
    if let Some(pid) = read_pid_state(name).and_then(|s| s.get("pid").and_then(|v| v.as_i64())) {
        if is_alive(pid as i32) {
            anyhow::bail!("{name} already running (pid {pid}); `helixir {name} stop` first");
        }
    }
    let exe = std::env::current_exe().context("current_exe")?;
    let log = helixir_dir()?.join(format!("{name}.log"));
    let out = OpenOptions::new().create(true).append(true).open(&log)?;
    let err = out.try_clone()?;

    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));
    // Detach from the controlling terminal so it survives the shell closing.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let pid = cmd.spawn().context("spawn detached process")?.id();

    let mut state = serde_json::json!({
        "pid": pid,
        "started_at": chrono::Utc::now().to_rfc3339(),
        "log": log.display().to_string(),
    });
    if let (Some(obj), Some(more)) = (state.as_object_mut(), extra.as_object()) {
        for (k, v) in more {
            obj.insert(k.clone(), v.clone());
        }
    }
    std::fs::write(pid_file(name)?, serde_json::to_string_pretty(&state)?)?;
    Ok((pid, log))
}

/// Detached background processes need setsid/pre_exec — unix-only. On
/// Windows the foreground variants (`helixir daemon run`, `helixir gateway`
/// in its own terminal) cover the same ground.
#[cfg(not(unix))]
fn spawn_detached(name: &str, _args: &[&str], _extra: serde_json::Value) -> Result<(u32, PathBuf)> {
    anyhow::bail!(
        "`helixir {name} start` (detached background process) is unix-only; on Windows run the foreground variant (e.g. `helixir daemon run`) in its own terminal"
    )
}

/// SIGTERM the named background process and clean up its pid file.
#[cfg(unix)]
fn stop_process(name: &str) -> Result<()> {
    let Some(state) = read_pid_state(name) else {
        println!("{name} not running (no pid file)");
        return Ok(());
    };
    let pid = state
        .get("pid")
        .and_then(|v| v.as_i64())
        .context("pid file has no pid")? as i32;
    if is_alive(pid) {
        unsafe { libc::kill(pid, libc::SIGTERM) };
        println!("{name} stopped (pid {pid})");
    } else {
        println!("{name} already gone (stale pid {pid}); cleaned up");
    }
    std::fs::remove_file(pid_file(name)?).ok();
    Ok(())
}

#[cfg(not(unix))]
fn stop_process(name: &str) -> Result<()> {
    // No detached processes exist on Windows (see spawn_detached); just
    // clear any stale state file copied over from a unix machine.
    std::fs::remove_file(pid_file(name)?).ok();
    println!("{name} not running (background processes are unix-only on this platform)");
    Ok(())
}

fn daemon_start(
    user: &str,
    interval: u64,
    threshold: f64,
    max_seeds: usize,
    max_hops: usize,
    cadence: [(&str, Option<u64>); 4],
) -> Result<()> {
    let interval_s = interval.to_string();
    let threshold_s = threshold.to_string();
    let max_seeds_s = max_seeds.to_string();
    let max_hops_s = max_hops.to_string();
    let mut args: Vec<String> = vec![
        "daemon".into(),
        "run".into(),
        "--user".into(),
        user.into(),
        "--interval".into(),
        interval_s,
        "--threshold".into(),
        threshold_s,
        "--max-seeds".into(),
        max_seeds_s,
        "--max-hops".into(),
        max_hops_s,
    ];
    for (flag, v) in cadence {
        if let Some(v) = v {
            args.push(flag.into());
            args.push(v.to_string());
        }
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (pid, log) = spawn_detached(
        "daemon",
        &arg_refs,
        serde_json::json!({
            "user": user, "interval": interval, "threshold": threshold,
            "max_seeds": max_seeds, "max_hops": max_hops,
        }),
    )?;
    println!(
        "daemon started (pid {pid}) for '{user}', every {interval}s; log: {}",
        log.display()
    );
    Ok(())
}

/// Foreground watchdog loop: sample the substrate, alert/heal per config.
async fn watch_run(client: &HelixirClient, once: bool, interval: Option<u64>) -> Result<()> {
    let tooling = client.tooling();
    let watchdog = client.config().watchdog.clone();
    let period = interval.unwrap_or(watchdog.sample_interval_secs);
    let mut hygieia = helixir::agents::hygieia::Hygieia::new(tooling);
    println!(
        "hygieia: watching every {period}s (container: {}) — journal {}",
        if watchdog.container_name.is_empty() {
            "none configured"
        } else {
            &watchdog.container_name
        },
        helixir::agents::hygieia::journal_path().display()
    );
    loop {
        let db_ok = hygieia.check_db().await;
        hygieia.check_memory().await;
        hygieia.check_orphan_daemons().await;
        hygieia.check_storage_persistence().await;
        hygieia.run_backup_duty().await;
        if once {
            println!("tick: db={}", if db_ok { "ok" } else { "DOWN" });
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(period)).await;
    }
}

/// Detach `watch run` as a background service (pid file + log, like the daemon).
/// #75: install the watchdog as a login service so it survives reboots.
/// macOS: a launchd agent at ~/Library/LaunchAgents; Linux: a systemd user
/// unit. The service runs `helixir watch run` in the FOREGROUND — the init
/// system owns the lifecycle, so no pid file is involved.
fn watch_install() -> Result<()> {
    let exe = std::env::current_exe().context("resolve helixir binary path")?;
    let home = std::env::var("HOME").context("HOME not set")?;
    // The service pins THIS binary path. A target/ path gets overwritten by
    // rebuilds — and on macOS replacing a running executable in place gets
    // it SIGKILLed (the 2026-07-02 incident). Install from the promoted
    // binary instead.
    if exe.components().any(|c| c.as_os_str() == "target") {
        anyhow::bail!(
            "refusing to install a service pinned to a build directory ({}) — \
             install the promoted binary instead: ~/.helixir/bin/helixir watch install",
            exe.display()
        );
    }

    #[cfg(target_os = "macos")]
    {
        let dir = std::path::PathBuf::from(&home).join("Library/LaunchAgents");
        std::fs::create_dir_all(&dir)?;
        let plist = dir.join("com.helixir.watchdog.plist");
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.helixir.watchdog</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>watch</string>
    <string>run</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>{home}/.helixir/watchdog.out.log</string>
  <key>StandardErrorPath</key><string>{home}/.helixir/watchdog.err.log</string>
</dict>
</plist>
"#,
            exe = exe.display(),
            home = home,
        );
        std::fs::write(&plist, body)?;
        let loaded = std::process::Command::new("launchctl")
            .args(["load", "-w"])
            .arg(&plist)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        println!(
            "Installed launchd agent: {}\nlaunchctl load: {}\nLogs: ~/.helixir/watchdog.{{out,err}}.log",
            plist.display(),
            if loaded {
                "OK (runs now and at every login)"
            } else {
                "FAILED — run manually: launchctl load -w <plist>"
            }
        );
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        let dir = std::path::PathBuf::from(&home).join(".config/systemd/user");
        std::fs::create_dir_all(&dir)?;
        let unit = dir.join("helixir-watchdog.service");
        let body = format!(
            "[Unit]\nDescription=Helixir health watchdog\n\n[Service]\nExecStart={} watch run\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
            exe.display()
        );
        std::fs::write(&unit, body)?;
        let ok = std::process::Command::new("systemctl")
            .args(["--user", "enable", "--now", "helixir-watchdog.service"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        println!(
            "Installed systemd user unit: {}\nsystemctl enable --now: {}",
            unit.display(),
            if ok {
                "OK"
            } else {
                "FAILED — run manually: systemctl --user enable --now helixir-watchdog"
            }
        );
        return Ok(());
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        anyhow::bail!("watch install supports macOS (launchd) and Linux (systemd user units)");
    }
}

/// #75: remove the login service installed by `watch install`.
fn watch_uninstall() -> Result<()> {
    let home = std::env::var("HOME").context("HOME not set")?;

    #[cfg(target_os = "macos")]
    {
        let plist =
            std::path::PathBuf::from(&home).join("Library/LaunchAgents/com.helixir.watchdog.plist");
        if !plist.exists() {
            println!("Nothing installed ({} not found).", plist.display());
            return Ok(());
        }
        let _ = std::process::Command::new("launchctl")
            .args(["unload", "-w"])
            .arg(&plist)
            .status();
        std::fs::remove_file(&plist)?;
        println!("Removed {}.", plist.display());
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        let unit =
            std::path::PathBuf::from(&home).join(".config/systemd/user/helixir-watchdog.service");
        if !unit.exists() {
            println!("Nothing installed ({} not found).", unit.display());
            return Ok(());
        }
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "disable", "--now", "helixir-watchdog.service"])
            .status();
        std::fs::remove_file(&unit)?;
        println!("Removed {}.", unit.display());
        return Ok(());
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        anyhow::bail!("watch uninstall supports macOS and Linux");
    }
}

fn watch_start(interval: Option<u64>) -> Result<()> {
    let mut args: Vec<String> = vec!["watch".into(), "run".into()];
    if let Some(i) = interval {
        args.push("--interval".into());
        args.push(i.to_string());
    }
    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
    let (pid, log) = spawn_detached("watch", &args_ref, serde_json::json!({}))?;
    println!("watch started (pid {pid}); log: {}", log.display());
    Ok(())
}

/// Pretty-print the tail of Hygieia's health journal.
fn health_tail(n: usize) -> Result<()> {
    let path = helixir::agents::hygieia::journal_path();
    let body = std::fs::read_to_string(&path)
        .with_context(|| format!("no health journal yet at {}", path.display()))?;
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    println!(
        "health events (last {} of {}):",
        lines.len() - start,
        lines.len()
    );
    for line in &lines[start..] {
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(v) => println!(
                "  {}  {:>5}  {:<20}  {}",
                v.get("at")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .get(..16)
                    .unwrap_or(""),
                v.get("severity").and_then(|x| x.as_str()).unwrap_or(""),
                v.get("kind").and_then(|x| x.as_str()).unwrap_or(""),
                v.get("summary").and_then(|x| x.as_str()).unwrap_or("")
            ),
            Err(_) => println!("  {line}"),
        }
    }
    Ok(())
}

fn daemon_stop() -> Result<()> {
    stop_process("daemon")
}

fn daemon_status() -> Result<()> {
    let Some(state) = read_pid_state("daemon") else {
        println!("daemon: stopped (no pid file)");
        return Ok(());
    };
    let pid = state.get("pid").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    println!(
        "daemon: {}  pid={pid} user={} interval={}s started={}",
        if is_alive(pid) {
            "running"
        } else {
            "STALE (process gone)"
        },
        state.get("user").and_then(|v| v.as_str()).unwrap_or("?"),
        state.get("interval").and_then(|v| v.as_u64()).unwrap_or(0),
        state
            .get("started_at")
            .and_then(|v| v.as_str())
            .unwrap_or("?"),
    );
    if let Some(l) = state.get("log").and_then(|v| v.as_str()) {
        println!("  log: {l}");
    }
    if let Ok(body) = std::fs::read_to_string(journal_path()) {
        if let Some(last) = body
            .lines()
            .filter(|l| l.contains("\"agent\":\"daemon\""))
            .last()
        {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(last) {
                println!(
                    "  last pass: {} — {}",
                    v.get("ts").and_then(|x| x.as_str()).unwrap_or("?"),
                    v.get("detail").and_then(|x| x.as_str()).unwrap_or("")
                );
            }
        }
    }
    Ok(())
}

fn gateway_start(bind: &str, require_auth: bool) -> Result<()> {
    let mut args = vec!["gateway", "run", "--bind", bind];
    if require_auth {
        args.push("--require-auth");
    }
    let auth_enabled = helixir::core::config::HelixirConfig::from_env()
        .gateway
        .auth_token
        .is_some_and(|token| !token.is_empty());
    let (pid, log) = spawn_detached(
        "gateway",
        &args,
        serde_json::json!({
            "bind": bind,
            "auth_enabled": auth_enabled,
            "auth_required": require_auth,
        }),
    )?;
    println!(
        "gateway started (pid {pid}) at http://{bind}/mcp; log: {}",
        log.display()
    );
    Ok(())
}

fn gateway_status() -> Result<()> {
    let Some(state) = read_pid_state("gateway") else {
        println!("gateway: stopped (no pid file)");
        return Ok(());
    };
    let pid = state.get("pid").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let bind = state.get("bind").and_then(|v| v.as_str()).unwrap_or("?");
    println!(
        "gateway: {}  pid={pid} url=http://{bind}/mcp auth={} started={}",
        if is_alive(pid) {
            "running"
        } else {
            "STALE (process gone)"
        },
        if state
            .get("auth_required")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            "required"
        } else if state
            .get("auth_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            "enabled"
        } else {
            "disabled"
        },
        state
            .get("started_at")
            .and_then(|v| v.as_str())
            .unwrap_or("?"),
    );
    if let Some(l) = state.get("log").and_then(|v| v.as_str()) {
        println!("  log: {l}");
    }
    Ok(())
}

// --- activity journal (append-only JSONL; the daemon will share it) ---

fn journal_path() -> PathBuf {
    std::env::var("HELIXIR_AGENT_LOG")
        .unwrap_or_else(|_| "helixir-agent-activity.jsonl".to_string())
        .into()
}

fn journal(agent: &str, action: &str, detail: &str) {
    let entry = serde_json::json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "agent": agent,
        "action": action,
        "detail": detail,
    });
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(journal_path())
    {
        let _ = writeln!(f, "{entry}");
    }
}

// --- insight journal (Atropos output; separate JSONL) ---

fn insight_journal_path() -> PathBuf {
    std::env::var("HELIXIR_INSIGHT_LOG")
        .unwrap_or_else(|_| "helixir-insights.jsonl".to_string())
        .into()
}

fn write_insight(insight: &Insight) {
    if let Ok(line) = serde_json::to_string(insight) {
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(insight_journal_path())
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn insights_tail(n: usize) -> Result<()> {
    let path = insight_journal_path();
    let body = std::fs::read_to_string(&path)
        .with_context(|| format!("no insight journal yet at {}", path.display()))?;
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    println!(
        "insight journal (last {} of {}):",
        lines.len() - start,
        lines.len()
    );
    for line in &lines[start..] {
        if let Ok(ins) = serde_json::from_str::<Insight>(line) {
            println!(
                "  ★ value {:.2}  [{} hops, min PMI {:.2}, {}]  {}",
                ins.value,
                ins.hops,
                ins.min_pmi,
                ins.status,
                ins.category_path.join(" → ")
            );
            for w in ins.witnesses.iter().take(2) {
                println!("       · {} :: {}", w.link, w.snippet);
            }
        }
    }
    Ok(())
}

fn journal_tail(n: usize) -> Result<()> {
    let path = journal_path();
    let body = std::fs::read_to_string(&path)
        .with_context(|| format!("no journal yet at {}", path.display()))?;
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    println!(
        "agent activity (last {} of {}):",
        lines.len() - start,
        lines.len()
    );
    for line in &lines[start..] {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            println!(
                "  {}  {:>8}  {}  {}",
                v.get("ts").and_then(|x| x.as_str()).unwrap_or(""),
                v.get("agent").and_then(|x| x.as_str()).unwrap_or(""),
                v.get("action").and_then(|x| x.as_str()).unwrap_or(""),
                v.get("detail").and_then(|x| x.as_str()).unwrap_or(""),
            );
        }
    }
    Ok(())
}
