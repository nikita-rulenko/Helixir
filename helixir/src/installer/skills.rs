//! Safe installation of the canonical Helixir Agent Skill.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::ClientKind;

/// Install one canonical skill into every selected agent ecosystem.
pub fn install(source: &Path, home: &Path, clients: &[ClientKind]) -> io::Result<Vec<PathBuf>> {
    let content = fs::read(source)?;
    let destinations = clients
        .iter()
        .map(|client| match client {
            ClientKind::ClaudeCode => home.join(".claude/skills/helixir-memory/SKILL.md"),
            ClientKind::Codex | ClientKind::Cursor => {
                home.join(".agents/skills/helixir-memory/SKILL.md")
            }
        })
        .collect::<BTreeSet<_>>();
    let mut changed = Vec::new();
    for destination in destinations {
        if fs::read(&destination).ok().as_deref() == Some(content.as_slice()) {
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        if destination.exists() {
            let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S%.3f");
            fs::copy(
                &destination,
                PathBuf::from(format!("{}.bak.{stamp}", destination.display())),
            )?;
        }
        let temporary = PathBuf::from(format!(
            "{}.tmp.{}",
            destination.display(),
            std::process::id()
        ));
        fs::write(&temporary, &content)?;
        if let Err(error) = fs::rename(&temporary, &destination) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        changed.push(destination);
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn codex_and_cursor_share_one_canonical_agents_skill() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("helixir-skill-{stamp}"));
        let source = root.join("source/SKILL.md");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(
            &source,
            b"---\nname: helixir-memory\ndescription: test\n---\n",
        )
        .unwrap();
        let changed = install(
            &source,
            &root,
            &[
                ClientKind::Codex,
                ClientKind::Cursor,
                ClientKind::ClaudeCode,
            ],
        )
        .unwrap();
        assert_eq!(changed.len(), 2);
        assert!(
            root.join(".agents/skills/helixir-memory/SKILL.md")
                .is_file()
        );
        assert!(
            root.join(".claude/skills/helixir-memory/SKILL.md")
                .is_file()
        );
        assert!(
            install(&source, &root, &[ClientKind::Codex])
                .unwrap()
                .is_empty()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn canonical_skill_covers_rbac_swarm_and_charter_operations() {
        let skill = include_str!("../../skills/helixir-memory/SKILL.md");

        for required in [
            "RBAC is permanently enabled",
            "swarm_status",
            "resolve_contradiction",
            "pending_outcomes",
            "memory://rules",
            "helixir rbac dedup attach",
        ] {
            assert!(
                skill.contains(required),
                "canonical skill must mention {required}"
            );
        }
    }
}
