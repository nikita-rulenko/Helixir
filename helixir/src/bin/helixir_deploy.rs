use std::env;
use std::fs;
use std::path::PathBuf;

use clap::Parser;

/// Deploy the Helixir schema + named queries to a running HelixDB.
// #15: clap-parsed — -h is help (not host), --version exists, and an invalid
// --port errors out instead of silently falling back to 6969.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// HelixDB host.
    #[arg(long, default_value = "localhost")]
    host: String,
    /// HelixDB port.
    #[arg(short, long, default_value_t = 6969)]
    port: u16,
    /// Deploy only schema.hx.
    #[arg(long)]
    schema_only: bool,
    /// Deploy only queries.hx.
    #[arg(long)]
    queries_only: bool,
    /// Directory holding schema.hx / queries.hx.
    #[arg(short = 'd', long, default_value = "schema")]
    schema_dir: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let (host, port) = (args.host, args.port);
    let (schema_only, queries_only) = (args.schema_only, args.queries_only);
    let mut schema_dir = args.schema_dir;

    println!("🚀 Helixir Schema Deployment");
    println!("   Target: {}:{}", host, port);
    println!("   Schema dir: {}", schema_dir.display());
    println!();

    if !schema_dir.exists() {
        let exe_dir = env::current_exe()?.parent().unwrap().to_path_buf();
        let alt_schema_dir = exe_dir.join("schema");
        if alt_schema_dir.exists() {
            schema_dir = alt_schema_dir;
        } else {
            eprintln!("❌ Schema directory not found: {}", schema_dir.display());
            eprintln!("   Try: --schema-dir /path/to/schema");
            std::process::exit(1);
        }
    }

    let base_url = format!("http://{}:{}", host, port);
    let client = reqwest::blocking::Client::new();

    if !queries_only {
        let schema_file = schema_dir.join("schema.hx");
        if schema_file.exists() {
            println!("📦 Deploying schema...");
            let schema_content = fs::read_to_string(&schema_file)?;

            match deploy_schema(&client, &base_url, &schema_content) {
                Ok(_) => println!("   ✅ Schema deployed successfully"),
                Err(e) => {
                    eprintln!("   ❌ Schema deployment failed: {}", e);
                    if !queries_only {
                        std::process::exit(1);
                    }
                }
            }
        } else {
            eprintln!("   ⚠️  schema.hx not found, skipping");
        }
    }

    if !schema_only {
        let queries_file = schema_dir.join("queries.hx");
        if queries_file.exists() {
            println!("📦 Deploying queries...");
            let queries_content = fs::read_to_string(&queries_file)?;

            match deploy_queries(&client, &base_url, &queries_content) {
                Ok(_) => println!("   ✅ Queries deployed successfully"),
                Err(e) => {
                    eprintln!("   ❌ Queries deployment failed: {}", e);
                    std::process::exit(1);
                }
            }
        } else {
            eprintln!("   ⚠️  queries.hx not found, skipping");
        }
    }

    println!();
    println!("🎉 Deployment complete!");

    Ok(())
}

fn deploy_schema(
    client: &reqwest::blocking::Client,
    base_url: &str,
    content: &str,
) -> anyhow::Result<()> {
    let url = format!("{}/schema", base_url);
    let response = client
        .post(&url)
        .header("Content-Type", "text/plain")
        .body(content.to_string())
        .send()?;

    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        Err(anyhow::anyhow!("HTTP {}: {}", status, body))
    }
}

fn deploy_queries(
    client: &reqwest::blocking::Client,
    base_url: &str,
    content: &str,
) -> anyhow::Result<()> {
    let url = format!("{}/queries", base_url);
    let response = client
        .post(&url)
        .header("Content-Type", "text/plain")
        .body(content.to_string())
        .send()?;

    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        Err(anyhow::anyhow!("HTTP {}: {}", status, body))
    }
}
