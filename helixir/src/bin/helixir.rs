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
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use helixir::agents::atropos::Insight;
use helixir::agents::daemon::DaemonConfig;
use helixir::agents::orchestrator::PassConfig;
use helixir::core::HelixirClient;
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
    /// The Moira daemon — schedule full passes (foreground or background).
    Daemon {
        #[command(subcommand)]
        cmd: DaemonCmd,
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

// --- daemon background lifecycle (PID file in ~/.helixir) ---

fn helixir_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    let dir = PathBuf::from(home).join(".helixir");
    std::fs::create_dir_all(&dir).ok();
    Ok(dir)
}

fn pid_file() -> Result<PathBuf> {
    Ok(helixir_dir()?.join("daemon.pid"))
}

fn read_pid_state() -> Option<serde_json::Value> {
    let body = std::fs::read_to_string(pid_file().ok()?).ok()?;
    serde_json::from_str(&body).ok()
}

/// Signal 0 probes a pid's existence without delivering anything.
fn is_alive(pid: i32) -> bool {
    pid > 0 && unsafe { libc::kill(pid, 0) == 0 }
}

fn daemon_start(
    user: &str,
    interval: u64,
    threshold: f64,
    max_seeds: usize,
    max_hops: usize,
) -> Result<()> {
    if let Some(pid) = read_pid_state().and_then(|s| s.get("pid").and_then(|v| v.as_i64())) {
        if is_alive(pid as i32) {
            anyhow::bail!("daemon already running (pid {pid}); `helixir daemon stop` first");
        }
    }

    let exe = std::env::current_exe().context("current_exe")?;
    let log = helixir_dir()?.join("daemon.log");
    let out = OpenOptions::new().create(true).append(true).open(&log)?;
    let err = out.try_clone()?;

    let mut cmd = Command::new(exe);
    cmd.args([
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
    ])
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
    let pid = cmd.spawn().context("spawn detached daemon")?.id();

    let state = serde_json::json!({
        "pid": pid, "user": user, "interval": interval, "threshold": threshold,
        "max_seeds": max_seeds, "max_hops": max_hops,
        "started_at": chrono::Utc::now().to_rfc3339(), "log": log.display().to_string(),
    });
    std::fs::write(pid_file()?, serde_json::to_string_pretty(&state)?)?;
    println!("daemon started (pid {pid}) for '{user}', every {interval}s; log: {}", log.display());
    Ok(())
}

fn daemon_stop() -> Result<()> {
    let Some(state) = read_pid_state() else {
        println!("daemon not running (no pid file)");
        return Ok(());
    };
    let pid = state.get("pid").and_then(|v| v.as_i64()).context("pid file has no pid")? as i32;
    if is_alive(pid) {
        unsafe { libc::kill(pid, libc::SIGTERM) };
        println!("daemon stopped (pid {pid})");
    } else {
        println!("daemon already gone (stale pid {pid}); cleaned up");
    }
    std::fs::remove_file(pid_file()?).ok();
    Ok(())
}

fn daemon_status() -> Result<()> {
    let Some(state) = read_pid_state() else {
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
