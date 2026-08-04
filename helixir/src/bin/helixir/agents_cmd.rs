use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn daemon_run(
    client: &HelixirClient,
    user: String,
    interval: u64,
    once: bool,
    threshold: f64,
    max_seeds: usize,
    max_hops: usize,
    cadence: [Option<u64>; 4],
) -> Result<()> {
    let admin = privileged(client).await?;
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
    admin
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

pub(crate) async fn pipeline_run(
    client: &HelixirClient,
    user: &str,
    threshold: f64,
    max_seeds: usize,
    max_hops: usize,
) -> Result<()> {
    let admin = privileged(client).await?;
    let cfg = PassConfig {
        grow_threshold: threshold,
        max_seeds,
        max_hops,
        ..PassConfig::default()
    };
    println!("Orchestrated pass for '{user}' (Clotho → Lachesis → Atropos)...");
    let run = admin.orchestrator().full_pass(user, &cfg).await?;
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

pub(crate) async fn atropos_run(
    client: &HelixirClient,
    limit: i64,
    max_seeds: usize,
    max_hops: usize,
) -> Result<()> {
    let admin = privileged(client).await?;
    let candidates = admin.tooling().list_categories(limit).await?;
    let universe = resolve_universe(client, None).await?;
    let seeds: Vec<(String, String)> = candidates.iter().take(max_seeds).cloned().collect();
    println!(
        "Atropos curating from {} seeds over {} candidates (N={universe})...",
        seeds.len(),
        candidates.len()
    );
    let insights = admin
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

pub(crate) async fn swarm_prune(client: &HelixirClient, agent_id: &str, yes: bool) -> Result<()> {
    if !yes {
        println!(
            "Refusing to prune '{agent_id}' without --yes.\n\
             This deletes the presence row AND its AGENT_CREATED provenance \
             edges — meant for true junk (test agents, renamed identities). \
             A merely-stale agent is already flagged in swarm_status."
        );
        return Ok(());
    }
    privileged(client)
        .await?
        .db()
        .execute_query::<serde_json::Value, _>(
            "dropPresenceByAgentId",
            &serde_json::json!({"agent_id": agent_id}),
        )
        .await?;
    println!("Pruned presence row for '{agent_id}'.");
    Ok(())
}

pub(crate) async fn charter_review(client: &HelixirClient) -> Result<()> {
    let admin = privileged(client).await?;
    let tooling = admin.tooling();
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

pub(crate) async fn categories(client: &HelixirClient, limit: i64) -> Result<()> {
    let admin = privileged(client).await?;
    let cats = admin.tooling().list_categories(limit).await?;
    let mut rows = Vec::with_capacity(cats.len());
    for (id, name) in cats {
        let n = admin.tooling().category_member_ids(&id).await?.len();
        rows.push((n, name, id));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    println!("{} categories (by member count):", rows.len());
    for (n, name, id) in &rows {
        println!("  {n:>6}  {name}   [{id}]");
    }
    Ok(())
}

pub(crate) async fn clotho_seed(client: &HelixirClient) -> Result<()> {
    let admin = privileged(client).await?;
    let n = admin.clotho().seed_dictionary().await?;
    println!("seeded {n} categories");
    journal("clotho", "seed", &format!("ensured {n} categories"));
    Ok(())
}

pub(crate) async fn clotho_tag(
    client: &HelixirClient,
    user: &str,
    limit: i64,
    threshold: f64,
    top_k: i64,
) -> Result<()> {
    let admin = privileged(client).await?;
    let mems = admin.tooling().list_user_memories(user, limit).await?;
    println!(
        "Clotho tagging {} memories for '{user}' (bar {threshold})...",
        mems.len()
    );
    let (mut tags, mut escalations, mut tagged_mems) = (0usize, 0usize, 0usize);
    for (id, content) in &mems {
        let outcome = admin
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

pub(crate) async fn clotho_grow(
    client: &HelixirClient,
    user: &str,
    limit: i64,
    threshold: f64,
) -> Result<()> {
    let admin = privileged(client).await?;
    let mems = admin.tooling().list_user_memories(user, limit).await?;
    println!(
        "Clotho grow-pass over {} memories for '{user}' (bar {threshold}); minting on miss...",
        mems.len()
    );
    let s = admin.clotho().grow_pass(&mems, threshold).await?;
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

pub(crate) async fn lachesis_pmi(
    client: &HelixirClient,
    cat_a: &str,
    cat_b: &str,
    universe: Option<usize>,
) -> Result<()> {
    let admin = privileged(client).await?;
    let universe = resolve_universe(client, universe).await?;
    let p = admin.lachesis().subset_pmi(cat_a, cat_b, universe).await?;
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

pub(crate) async fn lachesis_route(
    client: &HelixirClient,
    seed: &str,
    universe: Option<usize>,
    max_hops: usize,
) -> Result<()> {
    let admin = privileged(client).await?;
    let universe = resolve_universe(client, universe).await?;
    let candidates = admin.tooling().list_categories(500).await?;
    let hypo = admin
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

pub(crate) async fn chain(
    client: &HelixirClient,
    actor: &str,
    user: &str,
    topic: &str,
    max_hops: usize,
) -> Result<()> {
    match client
        .longest_chain_as(actor, topic, user, max_hops)
        .await?
    {
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
pub(crate) async fn resolve_universe(
    client: &HelixirClient,
    universe: Option<usize>,
) -> Result<usize> {
    match universe {
        Some(u) => Ok(u),
        None => Ok(privileged(client)
            .await?
            .tooling()
            .total_memory_count(1_000_000)
            .await?
            .max(1)),
    }
}

// --- setup wizard: configure + wire the MCP server into agent clients ---
