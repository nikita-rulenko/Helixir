use super::*;

pub(crate) async fn merge_run(client: &HelixirClient, limit: i64, threshold: f64) -> Result<()> {
    use helixir::agents::atropos::Atropos;
    println!("Paraphrase backstop (#43/#55) — collective scan (cosine ≥ {threshold}) …");
    let admin = privileged(client).await?;
    let atropos = Atropos::new(admin.tooling());
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

pub(crate) async fn backfill(client: &HelixirClient, limit: i64) -> Result<()> {
    let admin = privileged(client).await?;
    println!("Backfilling content_key fingerprints (#43 migration)…");
    let (scanned, updated) = admin
        .tooling()
        .backfill_content_keys(limit)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!(
        "Scanned {scanned} memories — stamped {updated} new fingerprints (the rest were already keyed)."
    );
    Ok(())
}

pub(crate) async fn debt(
    client: &HelixirClient,
    user: &str,
    limit: i64,
    reconcile: bool,
) -> Result<()> {
    use helixir::agents::atropos::reconcile::{DisputeKind, classify};

    if reconcile {
        let admin = privileged(client).await?;
        let s = admin
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

    let admin = privileged(client).await?;
    let open = admin
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
pub(crate) fn machine_host(explicit: &str) -> String {
    if !explicit.is_empty() {
        return explicit.to_string();
    }
    std::env::var("HELIXIR_HOST_LABEL")
        .or_else(|_| std::env::var("HOSTNAME"))
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "unknown".to_string())
}

pub(crate) fn human_age(secs: i64) -> String {
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

pub(crate) fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!(
            "{}…",
            s.chars().take(n.saturating_sub(1)).collect::<String>()
        )
    }
}

pub(crate) async fn heartbeat(
    client: &HelixirClient,
    agent: &str,
    role: &str,
    host: &str,
    status: &str,
) -> Result<()> {
    let host = machine_host(host);
    privileged(client)
        .await?
        .tooling()
        .register_or_heartbeat(agent, role, &host, status)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("✓ heartbeat: {agent} ({role}) on {host} — {status}");
    journal("swarm", "heartbeat", &format!("{agent}@{host}:{status}"));
    Ok(())
}

pub(crate) async fn swarm(client: &HelixirClient, window: Option<u64>) -> Result<()> {
    let window = window.unwrap_or(client.config().swarm.active_window_secs);
    let now = chrono::Utc::now();
    let mut roster = privileged(client)
        .await?
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
        "     {:<22} {:<11} {:<16} {:<7} status",
        "agent", "role", "host", "age"
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
