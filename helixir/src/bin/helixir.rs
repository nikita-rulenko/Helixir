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
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
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
