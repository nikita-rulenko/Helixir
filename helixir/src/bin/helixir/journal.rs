use super::*;

pub(crate) fn journal_path() -> PathBuf {
    std::env::var("HELIXIR_AGENT_LOG")
        .unwrap_or_else(|_| "helixir-agent-activity.jsonl".to_string())
        .into()
}

pub(crate) fn journal(agent: &str, action: &str, detail: &str) {
    let entry = serde_json::json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "agent": agent,
        "action": action,
        "detail": detail,
    });
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(journal_path())
    {
        let _ = writeln!(f, "{entry}");
    }
}

// --- insight journal (Atropos output; separate JSONL) ---

pub(crate) fn insight_journal_path() -> PathBuf {
    std::env::var("HELIXIR_INSIGHT_LOG")
        .unwrap_or_else(|_| "helixir-insights.jsonl".to_string())
        .into()
}

pub(crate) fn write_insight(insight: &Insight) {
    if let Ok(line) = serde_json::to_string(insight)
        && let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(insight_journal_path())
    {
        let _ = writeln!(f, "{line}");
    }
}

pub(crate) fn insights_tail(n: usize) -> Result<()> {
    let path = insight_journal_path();
    let body = std::fs::read_to_string(&path)
        .with_context(|| format!("no insight journal yet at {}", path.display()))?;
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    println!(
        "insight journal (last {} of {}):",
        lines.len() - start,
        lines.len()
    );
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

pub(crate) fn journal_tail(n: usize) -> Result<()> {
    let path = journal_path();
    let body = std::fs::read_to_string(&path)
        .with_context(|| format!("no journal yet at {}", path.display()))?;
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    println!(
        "agent activity (last {} of {}):",
        lines.len() - start,
        lines.len()
    );
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

#[cfg(test)]
mod rbac_cli_tests {
    use super::{Cli, Cmd, require_rbac_admin};
    use clap::Parser;
    use helixir::core::rbac::{RbacPolicy, Role};
    use std::path::PathBuf;

    #[test]
    fn parses_group_grant_and_status_commands() {
        let cli = Cli::try_parse_from([
            "helixir", "rbac", "grant", "--user", "alice", "--role", "worker", "--group", "alpha",
        ])
        .expect("valid rbac grant syntax");
        assert!(matches!(cli.cmd, Cmd::Rbac { .. }));

        let cli = Cli::try_parse_from(["helixir", "rbac", "status", "--json"])
            .expect("valid rbac status syntax");
        assert!(matches!(cli.cmd, Cmd::Rbac { .. }));

        let cli = Cli::try_parse_from([
            "helixir",
            "rbac",
            "dedup",
            "attach",
            "--group",
            "backend",
            "--dedup-group",
            "development",
        ])
        .expect("valid dedup attach syntax");
        assert!(matches!(cli.cmd, Cmd::Rbac { .. }));
    }

    #[test]
    fn management_requires_global_admin_only_when_rbac_is_enabled() {
        let mut enabled = RbacPolicy {
            enabled: true,
            ..Default::default()
        };
        enabled.assign_global("root", Role::Admin);

        assert!(require_rbac_admin(&enabled, "root", "group management").is_ok());
        assert!(require_rbac_admin(&enabled, "worker", "group management").is_err());
        assert!(require_rbac_admin(&RbacPolicy::default(), "worker", "group management").is_ok());
    }

    #[test]
    fn grant_cannot_spoof_actor_with_removed_cli_flag() {
        assert!(
            Cli::try_parse_from([
                "helixir", "rbac", "grant", "--user", "alice", "--role", "worker", "--group",
                "alpha", "--actor", "root",
            ])
            .is_err()
        );
    }

    #[test]
    fn onboard_non_interactive_model_flags_are_deterministic() {
        let cli = Cli::try_parse_from([
            "helixir",
            "onboard",
            "--non-interactive",
            "--local-llm-model",
            "qwen2.5:7b",
            "--dry-run",
        ])
        .expect("valid deterministic onboarding syntax");
        let Cmd::Onboard {
            non_interactive,
            dry_run,
            models,
            ..
        } = cli.cmd
        else {
            panic!("expected onboard command")
        };
        assert!(non_interactive);
        assert!(dry_run);
        assert_eq!(models.local_llm_model.as_deref(), Some("qwen2.5:7b"));

        let without_local_llm = Cli::try_parse_from([
            "helixir",
            "onboard",
            "--non-interactive",
            "--no-local-llm",
            "--dry-run",
        ])
        .expect("valid syntax without a local fallback LLM");
        assert!(matches!(
            without_local_llm.cmd,
            Cmd::Onboard { models, .. } if models.no_local_llm
        ));

        assert!(
            Cli::try_parse_from([
                "helixir",
                "onboard",
                "--local-llm-model",
                "qwen2.5:7b",
                "--no-local-llm",
            ])
            .is_err()
        );

        let remote = Cli::try_parse_from([
            "helixir",
            "onboard",
            "--non-interactive",
            "--remote-embeddings",
            "--embedding-provider",
            "openai",
            "--embedding-model",
            "text-embedding-3-small",
            "--embedding-url",
            "https://example.invalid/v1",
            "--dry-run",
        ])
        .expect("valid explicit remote embedding syntax");
        assert!(matches!(
            remote.cmd,
            Cmd::Onboard { models, .. }
                if models.remote_embeddings
                    && models.embedding_provider.as_deref() == Some("openai")
                    && models.embedding_model.as_deref() == Some("text-embedding-3-small")
        ));

        assert!(
            Cli::try_parse_from(["helixir", "onboard", "--embedding-provider", "openai",]).is_err()
        );
    }

    #[test]
    fn cli_modules_stay_within_the_500_line_budget() {
        let bin_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/bin");
        let mut files = vec![bin_dir.join("helixir.rs")];
        files.extend(
            std::fs::read_dir(bin_dir.join("helixir"))
                .unwrap()
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.extension().is_some_and(|ext| ext == "rs")),
        );
        for path in files {
            let lines = std::fs::read_to_string(&path).unwrap().lines().count();
            assert!(
                lines <= 500,
                "{} has {lines} lines; CLI modules are capped at 500",
                path.display()
            );
        }
    }
}
