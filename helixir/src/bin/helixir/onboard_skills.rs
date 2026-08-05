use super::*;

pub(crate) fn install_agent_skills(
    clients: &[helixir::installer::ClientKind],
) -> std::result::Result<(), String> {
    let home = PathBuf::from(
        std::env::var("HOME")
            .map_err(|_| "HOME is required to install agent skills".to_string())?,
    );
    let source = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .map(|parent| parent.join("skills/helixir-memory/SKILL.md"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills/helixir-memory/SKILL.md")
        });
    helixir::installer::skills::install(&source, &home, clients)
        .map_err(|error| error.to_string())?;
    Ok(())
}
