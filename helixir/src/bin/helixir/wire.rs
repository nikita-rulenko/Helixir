use super::*;

pub(crate) fn wire_entry_to_clients(
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
        println!(
            "{}",
            if dry_run {
                "\n(dry-run — nothing was written.)"
            } else {
                "\nDone."
            }
        );
        return Ok(());
    }

    let targets = client_targets();
    let selected: Vec<(String, PathBuf)> = if interactive {
        let labels: Vec<String> = targets
            .iter()
            .map(|(n, p)| {
                format!(
                    "{n}  [{}]{}",
                    p.display(),
                    if p.exists() { "" } else { " (new)" }
                )
            })
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
