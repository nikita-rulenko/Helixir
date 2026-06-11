//! End-to-end latency benchmark for **Helixir** (not raw HelixDB HTTP).
//!
//! Exercises the same paths as MCP:
//! - [`helixir::HelixirClient::search`] → embedding + [`SearchEngine`] (vector / BM25 / traversal per config).
//! - [`helixir::HelixirClient::add`] → full `add_memory` pipeline (LLM extract, embed, decision, graph).
//!
//! **Setup:** point `HELIX_HOST` / `HELIX_PORT` at your HelixDB instance; set LLM and embedding env vars
//! like for `helixir-mcp`. Optional graph bulk-load (Ansible + scripts) is only corpus prep — this binary
//! is what you use to compare `HELIXIR_RETRIEVAL_PROFILE` / HelixDB feature flags.
//!
//! ```text
//! cargo build --release -p helixir --bin helixir-bench
//! HELIX_HOST=10.0.0.5 HELIX_EMBEDDING_URL=http://localhost:11434 \
//!   ./target/release/helixir-bench --iterations 25 --scope all
//! ```

use std::time::Instant;

use anyhow::{Context, Result};
use helixir::core::{HelixirClient, RetrievalProfile};
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
struct CaseStats {
    n: usize,
    mean_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
}

#[derive(Serialize)]
struct BenchReport {
    helix_host: String,
    helix_port: u16,
    retrieval_profile: String,
    iterations: u32,
    warmup: u32,
    query: String,
    user_id: String,
    scope: String,
    limit: usize,
    cases: serde_json::Value,
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let n = sorted.len();
    let k = (n - 1) as f64 * p / 100.0;
    let f = k.floor() as usize;
    let c = k.ceil() as usize;
    if f == c {
        return sorted[f.min(n - 1)];
    }
    sorted[f] + (sorted[c.min(n - 1)] - sorted[f]) * (k - f as f64)
}

fn summarize(samples: &[f64]) -> CaseStats {
    let mut s = samples.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let sum: f64 = s.iter().sum();
    CaseStats {
        n: s.len(),
        mean_ms: if s.is_empty() { 0.0 } else { (sum / s.len() as f64) * 1000.0 },
        p50_ms: percentile(&s, 50.0) * 1000.0,
        p95_ms: percentile(&s, 95.0) * 1000.0,
    }
}

struct SearchBenchParams<'a> {
    query: &'a str,
    user_id: &'a str,
    limit: usize,
    mode: &'a str,
    scope: &'a str,
    warmup: u32,
    iterations: u32,
    /// Append a per-iteration suffix to the query so neither the embedding
    /// cache nor the traversal cache can short-circuit the measurement.
    vary_query: bool,
}

async fn bench_search(client: &HelixirClient, p: SearchBenchParams<'_>) -> Result<CaseStats> {
    let mut buf = Vec::with_capacity(p.iterations as usize);
    let total = p.warmup + p.iterations;
    for i in 0..total {
        let query = if p.vary_query {
            format!("{} {} variant {i}", p.query, p.mode)
        } else {
            p.query.to_string()
        };
        let t0 = Instant::now();
        let _ = client
            .search(
                &query,
                p.user_id,
                Some(p.limit),
                Some(p.mode),
                None,
                None,
                Some(p.scope),
            )
            .await
            .with_context(|| format!("search mode={} scope={}", p.mode, p.scope))?;
        let dt = t0.elapsed().as_secs_f64();
        if i >= p.warmup {
            buf.push(dt);
        }
    }
    Ok(summarize(&buf))
}

async fn bench_add(client: &HelixirClient, user_id: &str, warmup: u32, iterations: u32) -> Result<CaseStats> {
    let mut buf = Vec::with_capacity(iterations as usize);
    let total = warmup + iterations;
    for i in 0..total {
        let text = format!(
            "Bench add {}: isolated fact for latency — uuid {}",
            i,
            Uuid::new_v4()
        );
        let t0 = Instant::now();
        let _ = client
            .add(&text, user_id, None, None)
            .await
            .context("add_memory pipeline")?;
        let dt = t0.elapsed().as_secs_f64();
        if i >= warmup {
            buf.push(dt);
        }
    }
    Ok(summarize(&buf))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let mut iterations: u32 = 30;
    let mut warmup: u32 = 5;
    let mut query = "HelixDB Ansible retrieval benchmark hybrid".to_string();
    let mut user_id = "bench_user_00".to_string();
    let mut scope = "all".to_string();
    let mut limit: usize = 15;
    let mut modes: Vec<String> = vec!["contextual".to_string(), "deep".to_string()];
    let mut skip_add = false;
    let mut skip_search = false;
    let mut vary_query = false;
    let mut chain_probe = false;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--help" | "-h" => {
                eprintln!(
                    r#"helixir-bench — HelixirClient search + add_memory timings

  --iterations N   (default 30)
  --warmup N       (default 5)
  --query TEXT
  --user USER      (default bench_user_00)
  --scope S        personal|collective|all (default all)
  --limit N        (default 15)
  --modes MODES    comma-separated e.g. contextual,deep (default contextual,deep)
  --skip-add
  --skip-search
  --vary-query     append per-iteration suffix (defeats embedding/traversal caches)
"#
                );
                return Ok(());
            }
            "--iterations" => {
                iterations = args
                    .next()
                    .context("--iterations needs value")?
                    .parse()
                    .context("iterations u32")?;
            }
            "--warmup" => {
                warmup = args
                    .next()
                    .context("--warmup needs value")?
                    .parse()
                    .context("warmup u32")?;
            }
            "--query" => query = args.next().context("--query needs value")?,
            "--user" => user_id = args.next().context("--user needs value")?,
            "--scope" => scope = args.next().context("--scope needs value")?,
            "--limit" => {
                limit = args
                    .next()
                    .context("--limit needs value")?
                    .parse()
                    .context("limit usize")?;
            }
            "--modes" => {
                let s = args.next().context("--modes needs value")?;
                modes = s.split(',').map(|x| x.trim().to_string()).collect();
            }
            "--skip-add" => skip_add = true,
            "--skip-search" => skip_search = true,
            "--vary-query" => vary_query = true,
            "--chain-probe" => chain_probe = true,
            other => {
                anyhow::bail!("unknown arg: {other}");
            }
        }
    }

    let config = helixir::HelixirConfig::from_env();
    let host = config.host.clone();
    let port = config.port;
    let profile = RetrievalProfile::from_env();
    let profile_label = match profile {
        RetrievalProfile::Legacy => "legacy",
        RetrievalProfile::AlgoOpt => "algo_opt",
    };

    let client = HelixirClient::new(config).context("HelixirClient::new")?;
    client.initialize().await.context("initialize")?;

    if chain_probe {
        let t0 = Instant::now();
        let result = client
            .search_reasoning_chain(&query, &user_id, Some("both"), Some(5), Some(5))
            .await
            .context("search_reasoning_chain")?;
        eprintln!(
            "chain probe: {} chains, total_memories={}, deepest={}, took {}ms",
            result.chains.len(),
            result.total_memories,
            result.deepest_chain,
            t0.elapsed().as_millis()
        );
        for chain in &result.chains {
            eprintln!(
                "  seed {} -> {} nodes: {}",
                chain.seed.id,
                chain.nodes.len(),
                chain
                    .nodes
                    .iter()
                    .map(|n| format!("[{}] {}", n.relation, &n.memory_id))
                    .collect::<Vec<_>>()
                    .join(" ; ")
            );
        }
        return Ok(());
    }

    let mut cases = serde_json::Map::new();

    if !skip_search {
        for mode in &modes {
            let key = format!("search_{mode}_scope_{scope}");
            let stats = bench_search(
                &client,
                SearchBenchParams {
                    query: &query,
                    user_id: &user_id,
                    limit,
                    mode: mode.as_str(),
                    scope: &scope,
                    warmup,
                    iterations,
                    vary_query,
                },
            )
            .await?;
            cases.insert(key, serde_json::to_value(&stats)?);
        }
    }

    if !skip_add {
        let stats = bench_add(&client, &user_id, warmup, iterations).await?;
        cases.insert("add_memory_pipeline".to_string(), serde_json::to_value(&stats)?);
    }

    let report = BenchReport {
        helix_host: host,
        helix_port: port,
        retrieval_profile: profile_label.to_string(),
        iterations,
        warmup,
        query,
        user_id,
        scope,
        limit,
        cases: serde_json::Value::Object(cases),
    };

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
