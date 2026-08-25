use super::*;

pub(crate) async fn run() -> Result<()> {
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
    if let Cmd::ControlPlane { cmd } = &cli.cmd {
        return match cmd {
            ControlPlaneCmd::Install { image } => control_plane_install(image.as_deref()),
            ControlPlaneCmd::Status => control_plane_status(),
            ControlPlaneCmd::Uninstall => control_plane_uninstall(),
        };
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
                gateway_service_start(bind, *require_auth)
            }
            GatewayCmd::Stop => gateway_service_stop(),
            GatewayCmd::Status => gateway_service_status(),
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

    // `onboard` only detects the machine and constructs a typed plan; it does
    // not require a HelixDB connection. Platform executors are deliberately
    // kept behind the installer module so a future native UI can reuse them.
    if let Cmd::Onboard {
        non_interactive,
        dry_run,
        mode,
        models,
        backend,
        security,
    } = &cli.cmd
    {
        return onboard_run(
            !non_interactive,
            *dry_run,
            mode.clone(),
            models.clone(),
            backend.clone(),
            security.clone(),
        )
        .await;
    }

    if matches!(&cli.cmd, Cmd::ApplyInstallJson) {
        return apply_install_json().await;
    }

    if let Cmd::Doctor { json } = &cli.cmd {
        return doctor_run(*json).await;
    }

    if let Cmd::Web {
        bind,
        assets,
        token_file,
        prepare_token,
        no_open,
        containerized,
    } = &cli.cmd
    {
        let token_path =
            helixir::control_plane::session::token_path(token_file.as_deref(), *containerized);
        if *prepare_token {
            let _token = helixir::control_plane::session::load_or_create_token(&token_path)?;
            eprintln!("Helixir browser token: {}", token_path.display());
            return Ok(());
        }
        return helixir::control_plane::serve(helixir::control_plane::ControlPlaneConfig {
            bind: bind
                .parse()
                .context("parse web control-plane bind address")?,
            assets: assets.clone(),
            token_file: Some(token_path),
            open_browser: !no_open,
            containerized: *containerized,
        })
        .await;
    }

    if let Cmd::Supervisor { bind, token_file } = &cli.cmd {
        let token_path = token_file
            .clone()
            .unwrap_or_else(helixir::installer::supervisor::default_token_path);
        let token = helixir::installer::supervisor::load_or_create_token(&token_path)?;
        eprintln!("Helixir supervisor token: {}", token_path.display());
        return helixir::installer::supervisor::serve(
            helixir::installer::supervisor::SupervisorConfig {
                bind: bind.parse().context("parse supervisor bind address")?,
                token,
            },
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
        } => {
            let actor = rbac_actor();
            chain(&client, &actor, &user, &topic, max_hops).await?
        }
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
        Cmd::Onboard { .. } => unreachable!("onboard handled before client init"),
        Cmd::ApplyInstallJson => unreachable!("typed install apply handled before client init"),
        Cmd::Doctor { .. } => unreachable!("doctor handled before client init"),
        Cmd::Web { .. } => unreachable!("web handled before client init"),
        Cmd::Supervisor { .. } => unreachable!("supervisor handled before client init"),
        Cmd::ControlPlane { .. } => {
            unreachable!("control-plane lifecycle handled before client init")
        }
        Cmd::Rbac { cmd } => rbac_run(&client, cmd).await?,
        Cmd::Gateway { .. } => unreachable!("gateway handled before client init"),
        Cmd::Mode => unreachable!("mode handled before client init"),
        Cmd::Model { .. } => unreachable!("model handled before client init"),
    }
    Ok(())
}
