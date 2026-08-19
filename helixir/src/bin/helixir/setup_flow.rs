use super::*;

pub(crate) async fn probe_backend(host: &str, port: u16) -> bool {
    use std::net::ToSocketAddrs;

    let address = format!("{host}:{port}");
    tokio::task::spawn_blocking(move || {
        address
            .to_socket_addrs()
            .ok()
            .and_then(|mut addresses| addresses.next())
            .map(|address| {
                std::net::TcpStream::connect_timeout(&address, Duration::from_millis(500)).is_ok()
            })
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

/// Probe the local machine for a live HelixDB so a second client connects to the
/// existing backend instead of standing up a duplicate (the singleton rule). The
/// env-pinned port is tried first, then the common Helix ports.
pub(crate) async fn discover_backends() -> Vec<(String, u16)> {
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
pub(crate) async fn verify_backend(cfg: &SetupConfig) -> Result<()> {
    let port: u16 = cfg.port.parse().context("HelixDB port must be a number")?;
    if probe_backend(&cfg.host, port).await {
        Ok(())
    } else {
        anyhow::bail!(
            "no HelixDB listener at {}:{} (TCP probe timed out)",
            cfg.host,
            port
        )
    }
}

/// Best-effort primary LAN IP: open a UDP socket "toward" a public address and
/// read which local interface the OS would route through. Sends no packet.
pub(crate) fn lan_ip() -> Option<std::net::IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    let ip = sock.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// Interactive memory-mode picker for `helixir setup` when no mode was
/// stated via `--mode` or HELIXIR_MODE. Collective is the recommended default
/// (index 0): a person running the wizard is consciously joining the shared
/// memory, which is the point of the tool. Solo and Insights stay one keystroke
/// away, and the silent library default (HelixirConfig::new) remains Solo.
pub(crate) fn prompt_mode_recommendation() -> Result<MemoryMode> {
    let options = [
        "collective — shared memory across your agents (recommended)",
        "solo — private, single user, no cross-user behaviour",
        "insights — collective + the generative Moirai (advanced)",
    ];
    let idx = Select::new()
        .with_prompt("Memory mode")
        .default(0)
        .items(&options)
        .interact()?;
    Ok([
        MemoryMode::Collective,
        MemoryMode::Solo,
        MemoryMode::Insights,
    ][idx])
}

/// During setup, ensure the required local NLI model is present and loadable.
pub(crate) async fn ensure_setup_nli_model(dry_run: bool) -> Result<()> {
    use helixir::llm::nli;
    let s = nli::status();
    println!(
        "Helixir requires the local NLI safety model — variant {} for {}.",
        s.variant_for_host, s.host
    );
    if s.installed && nli::verify_readiness().is_ok() {
        println!(
            "  ✓ already installed and verified ({:.0} MB at {}).\n",
            s.onnx_bytes as f64 / 1e6,
            s.dir.display()
        );
        return Ok(());
    }
    if dry_run {
        println!("  (dry-run: would download ~90 MB)\n");
        return Ok(());
    }
    let bytes = nli::download(s.installed).await?;
    nli::verify_readiness().context("downloaded NLI model failed its readiness check")?;
    println!(
        "  ✓ fetched {:.0} MB — NLI model ready.\n",
        bytes as f64 / 1e6
    );
    Ok(())
}

pub(crate) async fn setup_run(
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
    println!("Memory mode: {mode_label} (HELIXIR_MODE).\n");

    // NLI is a required write-safety component in every memory mode.
    ensure_setup_nli_model(dry_run).await?;

    // Gateway mode short-circuits DB discovery: clients talk to the per-host
    // gateway over HTTP, which holds the HELIX_* config — they carry none.
    if let Some(gw) = gateway {
        let url = normalize_gateway_url(&gw);
        println!("Gateway mode — wiring clients to {url}");
        println!("  HTTP transport: clients carry no HELIX_* env; the gateway holds the config.");
        println!("  The memory mode lives on the GATEWAY process — start it with");
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
    if target.is_none() {
        let native_server = helixir::installer::clients::StdioServer::new(cfg.mcp_bin.clone());
        wire_native_clients(&native_server, interactive, dry_run)?;
    }
    let source = format!("helixir-mcp at {}", cfg.mcp_bin);
    wire_entry_to_clients(entry, target, interactive, dry_run, &source)
}

// Read-only onboarding discovery lives in the adjacent module.
