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
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use dialoguer::{Confirm, Input, MultiSelect};
use helixir::agents::atropos::Insight;
use helixir::agents::daemon::DaemonConfig;
use helixir::agents::orchestrator::PassConfig;
use helixir::core::HelixirClient;
use helixir::HelixClient;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "helixir", about = "Helixir agent control & monitoring (the Moirai)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
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
        #[arg(long, default_value_t = 90)]
        window: u64,
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
    /// background. Full network trust for v1 (no auth token).
    Gateway {
        #[command(subcommand)]
        cmd: GatewayCmd,
    },
    /// The Moira daemon — schedule full passes (foreground or background).
    Daemon {
        #[command(subcommand)]
        cmd: DaemonCmd,
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
    },
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
        #[arg(long, default_value = "0.0.0.0:8765")]
        bind: String,
    },
    /// Start a DETACHED background gateway. Writes a PID file; `stop` ends it.
    Start {
        #[arg(long, default_value = "0.0.0.0:8765")]
        bind: String,
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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "helixir=info".into()))
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
            } => return daemon_start(user, *interval, *threshold, *max_seeds, *max_hops),
            DaemonCmd::Stop => return daemon_stop(),
            DaemonCmd::Status => return daemon_status(),
            DaemonCmd::Run { .. } => {} // needs the client — fall through
        }
    }

    // Gateway: Run serves over HTTP (its own mcp-style client init); Start/Stop/
    // Status are process management (no DB) — all handled before the shared init.
    if let Cmd::Gateway { cmd } = &cli.cmd {
        return match cmd {
            GatewayCmd::Run { bind } => helixir::mcp::run_gateway(bind).await,
            GatewayCmd::Start { bind } => gateway_start(bind),
            GatewayCmd::Stop => stop_process("gateway"),
            GatewayCmd::Status => gateway_status(),
        };
    }

    // Setup configures files + client configs; no DB connection needed.
    if let Cmd::Setup {
        non_interactive,
        dry_run,
        gateway,
        target,
    } = &cli.cmd
    {
        return setup_run(!non_interactive, *dry_run, target.clone(), gateway.clone()).await;
    }

    let client = HelixirClient::from_env().context("from_env (set HELIX_* env)")?;
    client.initialize().await.context("initialize")?;

    match cli.cmd {
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
            } => daemon_run(&client, user, interval, once, threshold, max_seeds, max_hops).await?,
            _ => unreachable!("daemon start/stop/status handled before client init"),
        },
        Cmd::Setup { .. } => unreachable!("setup handled before client init"),
        Cmd::Gateway { .. } => unreachable!("gateway handled before client init"),
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
) -> Result<()> {
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
    println!("Clotho tagging {} memories for '{user}' (bar {threshold})...", mems.len());
    let (mut tags, mut escalations, mut tagged_mems) = (0usize, 0usize, 0usize);
    for (id, content) in &mems {
        let outcome = client.clotho().auto_tag(id, content, top_k, threshold).await?;
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
            println!("longest chain: {} hops, confidence {:.4}", n.hops, n.confidence);
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
        serde_json::from_str(&std::fs::read_to_string(path)?).unwrap_or_else(|_| serde_json::json!({}))
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
        println!("  [dry-run] {name}: would set helixir-local in {}", path.display());
        return Ok(());
    }
    if path.exists() {
        std::fs::copy(path, PathBuf::from(format!("{}.bak", path.display()))).ok();
    } else if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(path, serde_json::to_string_pretty(&root)?)?;
    println!("  ✓ {name}: wired helixir-local → {} (backup .bak)", path.display());
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
    let env_port: Option<u16> = std::env::var("HELIX_PORT").ok().and_then(|p| p.parse().ok());
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

async fn setup_run(
    interactive: bool,
    dry_run: bool,
    target: Option<String>,
    gateway: Option<String>,
) -> Result<()> {
    println!("Helixir setup — configure + wire its MCP server into your agent clients\n");

    // Gateway mode short-circuits DB discovery: clients talk to the per-host
    // gateway over HTTP, which holds the HELIX_* config — they carry none.
    if let Some(gw) = gateway {
        let url = normalize_gateway_url(&gw);
        println!("Gateway mode — wiring clients to {url}");
        println!("  HTTP transport: clients carry no HELIX_* env; the gateway holds the config.");
        println!("  (Make sure a gateway is running there: `helixir gateway start`.)\n");
        let entry = mcp_entry_gateway(&url);
        return wire_entry_to_clients(entry, target, interactive, dry_run, &format!("gateway {url}"));
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

    let cfg = gather_config(interactive && target.is_none(), found.into_iter().next())?;

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
                println!(
                    "  Other hosts join the same collective by setting their client's"
                );
                println!(
                    "  HELIX_HOST={ip} (full network trust assumed — no auth token yet).\n"
                );
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
        println!("{}", if dry_run { "\n(dry-run — nothing was written.)" } else { "\nDone." });
        return Ok(());
    }

    let targets = client_targets();
    let selected: Vec<(String, PathBuf)> = if interactive {
        let labels: Vec<String> = targets
            .iter()
            .map(|(n, p)| format!("{n}  [{}]{}", p.display(), if p.exists() { "" } else { " (new)" }))
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

async fn debt(client: &HelixirClient, user: &str, limit: i64, reconcile: bool) -> Result<()> {
    use helixir::agents::atropos::reconcile::{classify, DisputeKind};

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
    println!("Open contradiction debt for '{user}' — {} dispute(s):\n", open.len());
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
        format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>())
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

async fn swarm(client: &HelixirClient, window: u64) -> Result<()> {
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
fn is_alive(pid: i32) -> bool {
    pid > 0 && unsafe { libc::kill(pid, 0) == 0 }
}

/// Spawn `helixir <args>` as a detached background process (setsid), logging to
/// `~/.helixir/<name>.log` and recording a `<name>.pid` state file. Shared by
/// the daemon (#43) and the gateway (#42). Returns the child pid.
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

/// SIGTERM the named background process and clean up its pid file.
fn stop_process(name: &str) -> Result<()> {
    let Some(state) = read_pid_state(name) else {
        println!("{name} not running (no pid file)");
        return Ok(());
    };
    let pid = state.get("pid").and_then(|v| v.as_i64()).context("pid file has no pid")? as i32;
    if is_alive(pid) {
        unsafe { libc::kill(pid, libc::SIGTERM) };
        println!("{name} stopped (pid {pid})");
    } else {
        println!("{name} already gone (stale pid {pid}); cleaned up");
    }
    std::fs::remove_file(pid_file(name)?).ok();
    Ok(())
}

fn daemon_start(
    user: &str,
    interval: u64,
    threshold: f64,
    max_seeds: usize,
    max_hops: usize,
) -> Result<()> {
    let (pid, log) = spawn_detached(
        "daemon",
        &[
            "daemon",
            "run",
            "--user",
            user,
            "--interval",
            &interval.to_string(),
            "--threshold",
            &threshold.to_string(),
            "--max-seeds",
            &max_seeds.to_string(),
            "--max-hops",
            &max_hops.to_string(),
        ],
        serde_json::json!({
            "user": user, "interval": interval, "threshold": threshold,
            "max_seeds": max_seeds, "max_hops": max_hops,
        }),
    )?;
    println!("daemon started (pid {pid}) for '{user}', every {interval}s; log: {}", log.display());
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
        if is_alive(pid) { "running" } else { "STALE (process gone)" },
        state.get("user").and_then(|v| v.as_str()).unwrap_or("?"),
        state.get("interval").and_then(|v| v.as_u64()).unwrap_or(0),
        state.get("started_at").and_then(|v| v.as_str()).unwrap_or("?"),
    );
    if let Some(l) = state.get("log").and_then(|v| v.as_str()) {
        println!("  log: {l}");
    }
    if let Ok(body) = std::fs::read_to_string(journal_path()) {
        if let Some(last) = body.lines().filter(|l| l.contains("\"agent\":\"daemon\"")).last() {
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

fn gateway_start(bind: &str) -> Result<()> {
    let (pid, log) = spawn_detached(
        "gateway",
        &["gateway", "run", "--bind", bind],
        serde_json::json!({ "bind": bind }),
    )?;
    println!("gateway started (pid {pid}) at http://{bind}/mcp; log: {}", log.display());
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
        "gateway: {}  pid={pid} url=http://{bind}/mcp started={}",
        if is_alive(pid) { "running" } else { "STALE (process gone)" },
        state.get("started_at").and_then(|v| v.as_str()).unwrap_or("?"),
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
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(journal_path()) {
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
    println!("insight journal (last {} of {}):", lines.len() - start, lines.len());
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
    println!("agent activity (last {} of {}):", lines.len() - start, lines.len());
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
