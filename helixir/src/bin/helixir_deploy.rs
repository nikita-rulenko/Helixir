

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    
    
    let mut host = "localhost".to_string();
    let mut port = 6969u16;
    let mut schema_only = false;
    let mut queries_only = false;
    let mut schema_dir = PathBuf::from("schema");
    
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--host" | "-h" => {
                if i + 1 < args.len() {
                    host = args[i + 1].clone();
                    i += 1;
                }
            }
            "--port" | "-p" => {
                if i + 1 < args.len() {
                    port = args[i + 1].parse().unwrap_or(6969);
                    i += 1;
                }
            }
            "--schema-only" => schema_only = true,
            "--queries-only" => queries_only = true,
            "--schema-dir" | "-d" => {
                if i + 1 < args.len() {
                    schema_dir = PathBuf::from(&args[i + 1]);
                    i += 1;
                }
            }
            "--help" => {
                print_help();
                return Ok(());
            }
            _ => {}
        }
        i += 1;
    }
    
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

fn deploy_schema(client: &reqwest::blocking::Client, base_url: &str, content: &str) -> anyhow::Result<()> {
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

fn deploy_queries(client: &reqwest::blocking::Client, base_url: &str, content: &str) -> anyhow::Result<()> {
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

fn print_help() {
    println!(r#"
Helixir Schema Deployment CLI

USAGE:
    helixir-deploy [OPTIONS]

OPTIONS:
    -h, --host <HOST>       HelixDB host (default: localhost)
    -p, --port <PORT>       HelixDB port (default: 6969)
    -d, --schema-dir <DIR>  Schema directory (default: ./schema)
    --schema-only           Deploy only schema.hx
    --queries-only          Deploy only queries.hx
    --help                  Print this help

EXAMPLES:
    # Deploy to local HelixDB
    helixir-deploy

    # Deploy to remote server
    helixir-deploy --host 192.168.50.11 --port 6969

    # Deploy only queries (schema already exists)
    helixir-deploy --host myserver.com --queries-only

ENVIRONMENT:
    HELIX_HOST              Override default host
    HELIX_PORT              Override default port
"#);
}

