//! Canonical skill and AGENTS.md installation with marker-based, backup-safe updates.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::profile::{ClientProfile, home_dir};
use crate::registration::ClientKind;

const SKILL_SOURCE: &str = include_str!("../../helixir/skills/helixir-memory/SKILL.md");
const AGENTS_SOURCE: &str = include_str!("../../integration/AGENTS.md");
const START: &str = "<!-- helixir-client:start -->";
const END: &str = "<!-- helixir-client:end -->";

pub fn install(profile: &ClientProfile) -> Result<Vec<PathBuf>> {
    if !profile.project_root.is_dir() {
        bail!(
            "project root {} is not a directory",
            profile.project_root.display()
        );
    }
    let mut changed = Vec::new();
    let skill = render_skill(profile);
    for path in skill_paths(&profile.clients)? {
        if write_if_changed(&path, skill.as_bytes())? {
            changed.push(path);
        }
    }
    let agents_path = profile.project_root.join("AGENTS.md");
    if install_agents_block(&agents_path, &render_agents_block(profile))? {
        changed.push(agents_path);
    }
    Ok(changed)
}

pub fn verify(profile: &ClientProfile) -> Result<Vec<String>> {
    let mut failures = Vec::new();
    let expected_skill = render_skill(profile);
    for path in skill_paths(&profile.clients)? {
        match fs::read_to_string(&path) {
            Ok(actual) if actual == expected_skill => {}
            Ok(_) => failures.push(format!("stale skill: {}", path.display())),
            Err(_) => failures.push(format!("missing skill: {}", path.display())),
        }
    }
    let agents_path = profile.project_root.join("AGENTS.md");
    let expected_block = render_agents_block(profile);
    match fs::read_to_string(&agents_path) {
        Ok(actual) if actual.contains(&expected_block) => {}
        Ok(_) => failures.push(format!(
            "stale managed AGENTS block: {}",
            agents_path.display()
        )),
        Err(_) => failures.push(format!("missing AGENTS.md: {}", agents_path.display())),
    }
    Ok(failures)
}

fn skill_paths(clients: &[ClientKind]) -> Result<BTreeSet<PathBuf>> {
    let home = home_dir()?;
    Ok(clients
        .iter()
        .map(|client| match client {
            ClientKind::Claude => home.join(".claude/skills/helixir-memory/SKILL.md"),
            ClientKind::Codex | ClientKind::Cursor => {
                home.join(".agents/skills/helixir-memory/SKILL.md")
            }
        })
        .collect())
}

fn render_skill(profile: &ClientProfile) -> String {
    format!(
        "{SKILL_SOURCE}\n\n## Installed remote-client identity\n\n\
         This host connects to {gateway}. Use {principal} as `actor_id` and \
         {owner} as `user_id`. The principal is admitted through reserved \
         `onboarding`; after an administrator assigns working groups, pass the \
         concrete group id on writes. Never connect directly to the HelixDB port.\n",
        gateway = markdown_code(&profile.gateway_url),
        principal = markdown_code(&profile.principal_id),
        owner = markdown_code(&profile.owner_id),
    )
}

fn render_agents_block(profile: &ClientProfile) -> String {
    let owner_literal = serde_json::to_string(&profile.owner_id)
        .expect("serializing a Rust string to JSON cannot fail");
    let template = AGENTS_SOURCE
        .replace("user_id=\"claude\"", &format!("user_id={owner_literal}"))
        .replace(
            "actor_id=\"claude\"",
            &format!("actor_id=\"{}\"", profile.principal_id),
        )
        .replace(
            "agent_id=\"claude\"",
            &format!("agent_id=\"{}\"", profile.principal_id),
        );
    format!(
        "{START}\n\
         # Helixir remote memory — managed by helixir-client\n\n\
         Gateway: {gateway}  \n\
         RBAC principal (`actor_id`): {principal}  \n\
         Memory owner (`user_id`): {owner}\n\n\
         These installed values override example placeholders in the canonical \
         instructions below. Do not edit this managed block by hand; rerun \
         `helixir-client connect` to update it.\n\n\
         {template}\n\
         {END}",
        gateway = markdown_code(&profile.gateway_url),
        principal = markdown_code(&profile.principal_id),
        owner = markdown_code(&profile.owner_id),
    )
}

fn markdown_code(value: &str) -> String {
    format!(
        "<code>{}</code>",
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    )
}

fn install_agents_block(path: &Path, block: &str) -> Result<bool> {
    refuse_symlink(path)?;
    let existing = fs::read_to_string(path).unwrap_or_default();
    let rendered = match (existing.find(START), existing.find(END)) {
        (Some(start), Some(end)) if start <= end => {
            let suffix_start = end + END.len();
            format!(
                "{}{}{}",
                &existing[..start],
                block,
                &existing[suffix_start..]
            )
        }
        (None, None) if existing.trim().is_empty() => format!("{block}\n"),
        (None, None) => format!("{}\n\n{block}\n", existing.trim_end()),
        _ => bail!(
            "{} contains an incomplete helixir-client managed block",
            path.display()
        ),
    };
    write_if_changed(path, rendered.as_bytes())
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<bool> {
    refuse_symlink(path)?;
    if fs::read(path).is_ok_and(|existing| existing == bytes) {
        return Ok(false);
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{} has no parent", path.display()))?;
    fs::create_dir_all(parent)?;
    if path.exists() {
        let backup = PathBuf::from(format!(
            "{}.bak.{}",
            path.display(),
            chrono::Utc::now().format("%Y%m%d-%H%M%S%.3f")
        ));
        fs::copy(path, backup)?;
    }
    let temporary = PathBuf::from(format!("{}.tmp.{}", path.display(), std::process::id()));
    fs::write(&temporary, bytes)
        .with_context(|| format!("write temporary file {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("replace {}", path.display()))?;
    Ok(true)
}

fn refuse_symlink(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        bail!("refusing to replace symlink {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(root: &Path) -> ClientProfile {
        ClientProfile {
            gateway_url: "http://host:8765/mcp".to_string(),
            principal_id: "codex-laptop".to_string(),
            owner_id: "codex".to_string(),
            clients: vec![],
            project_root: root.to_path_buf(),
            token_env: "HELIXIR_GATEWAY_TOKEN".to_string(),
            installed_at: "2026-08-21T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn managed_agents_update_preserves_project_rules() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("AGENTS.md");
        fs::write(&path, "# Project rules\n\nKeep this.\n").unwrap();
        let first = render_agents_block(&profile(root.path()));
        assert!(install_agents_block(&path, &first).unwrap());
        assert!(!install_agents_block(&path, &first).unwrap());
        let mut changed = profile(root.path());
        changed.owner_id = "nikita".to_string();
        assert!(install_agents_block(&path, &render_agents_block(&changed)).unwrap());
        let saved = fs::read_to_string(path).unwrap();
        assert!(saved.contains("Keep this."));
        assert_eq!(saved.matches(START).count(), 1);
        assert!(saved.contains("<code>nikita</code>"));
    }

    #[test]
    fn identity_rendering_cannot_inject_markdown_or_tool_arguments() {
        let mut changed = profile(Path::new("/tmp"));
        changed.owner_id = "owner\"; actor_id=\"attacker<unsafe>".to_string();
        let rendered = render_agents_block(&changed);
        assert!(rendered.contains(r#"user_id="owner\"; actor_id=\"attacker<unsafe>""#));
        assert!(rendered.contains("<code>owner\"; actor_id=\"attacker&lt;unsafe&gt;</code>"));
    }
}
