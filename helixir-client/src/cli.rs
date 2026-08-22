//! Command-line orchestration for remote client connection and diagnosis.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use dialoguer::{Input, MultiSelect};

use crate::doctor;
use crate::gateway::{McpClient, normalize_gateway_url};
use crate::instructions;
use crate::profile::ClientProfile;
use crate::registration::{self, ClientKind};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Connect an agent host to a remote Helixir MCP gateway"
)]
pub struct Cli {
    #[arg(long, global = true, value_name = "FILE")]
    profile: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Connect this host, enroll its principal, and install agent integrations.
    Connect(ConnectArgs),
    /// Verify gateway, RBAC enrollment, MCP registrations, skill, and AGENTS.md.
    Doctor,
    /// Print the non-secret local client profile.
    Status,
}

#[derive(Debug, Args)]
struct ConnectArgs {
    /// Helixir gateway URL or host:port. This is the MCP endpoint, not a HelixDB port.
    #[arg(long)]
    gateway: Option<String>,
    /// Stable lower-case RBAC principal for this agent host.
    #[arg(long)]
    principal: Option<String>,
    /// Stable memory owner (`user_id`); defaults to the principal.
    #[arg(long)]
    owner: Option<String>,
    /// Project whose AGENTS.md receives the managed Helixir instructions.
    #[arg(long)]
    project: Option<PathBuf>,
    /// Agent client to configure; repeat for multiple clients.
    #[arg(long = "client", value_enum)]
    clients: Vec<ClientKind>,
    /// Environment variable containing an optional gateway bearer token.
    #[arg(long, default_value = "HELIXIR_GATEWAY_TOKEN")]
    token_env: String,
    /// Accept detected defaults without interactive questions.
    #[arg(long)]
    yes: bool,
    /// Replace an existing conflicting helixir-local MCP registration.
    #[arg(long)]
    replace: bool,
}

impl Cli {
    pub fn run(self) -> Result<()> {
        let profile_path = self.profile.unwrap_or(ClientProfile::default_path()?);
        match self.command {
            Command::Connect(args) => connect(args, &profile_path),
            Command::Doctor => doctor::run(&ClientProfile::load(&profile_path)?),
            Command::Status => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&ClientProfile::load(&profile_path)?)?
                );
                Ok(())
            }
        }
    }
}

fn connect(args: ConnectArgs, profile_path: &std::path::Path) -> Result<()> {
    let gateway_raw = required_value(
        args.gateway,
        args.yes,
        "Helixir gateway (host:port or URL)",
        None,
    )?;
    let gateway_url = normalize_gateway_url(&gateway_raw)?;
    let principal = required_value(
        args.principal,
        args.yes,
        "Stable RBAC principal",
        suggested_principal().as_deref(),
    )?;
    validate_principal(&principal)?;
    let owner = optional_value(args.owner, args.yes, "Memory owner (user_id)", &principal)?;
    validate_owner(&owner)?;
    let project = args
        .project
        .unwrap_or(std::env::current_dir()?)
        .canonicalize()
        .context("resolve project root")?;
    if !project.is_dir() {
        bail!("project root {} is not a directory", project.display());
    }
    let clients = select_clients(args.clients, args.yes)?;
    if clients.is_empty() {
        bail!("select at least one installed client with --client");
    }
    if !args.token_env.is_empty() && args.token_env.contains('=') {
        bail!("--token-env accepts an environment variable name, not a token value");
    }

    println!("Connecting to {gateway_url} …");
    let token = doctor::token_from_env(&args.token_env);
    let mut gateway = McpClient::connect(&gateway_url, token.clone())?;
    let tools = gateway.tool_names()?;
    let missing = doctor::missing_required_tools(&tools);
    if !missing.is_empty() {
        bail!(
            "gateway is incompatible: missing required MCP tools {}",
            missing.join(", ")
        );
    }
    let enrollment = gateway.enroll_client(&principal)?;
    if enrollment.principal_id != principal {
        bail!("gateway enrolled a different principal");
    }
    if enrollment.roles.is_empty() {
        bail!(
            "principal {principal} is registered but has no active role; ask a Helixir admin to restore group access"
        );
    }
    println!(
        "RBAC: {} in {} ({})",
        enrollment.principal_id,
        enrollment.group_id,
        enrollment.roles.join(", ")
    );

    for client in &clients {
        registration::register(
            *client,
            &gateway_url,
            token.as_ref().map(|_| args.token_env.as_str()),
            args.replace,
        )?;
        println!("Configured {} → {gateway_url}", client.label());
    }
    let profile = ClientProfile {
        gateway_url,
        principal_id: principal,
        owner_id: owner,
        clients,
        project_root: project,
        token_env: args.token_env,
        installed_at: chrono::Utc::now().to_rfc3339(),
    };
    for path in instructions::install(&profile)? {
        println!("Installed {}", path.display());
    }
    profile.save(profile_path)?;
    doctor::run(&profile)?;
    println!("Helixir client is ready. Restart the selected agent clients once.");
    Ok(())
}

fn required_value(
    value: Option<String>,
    non_interactive: bool,
    prompt: &str,
    default: Option<&str>,
) -> Result<String> {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        return Ok(value.trim().to_string());
    }
    if non_interactive {
        bail!(
            "--{} is required with --yes",
            prompt.to_ascii_lowercase().replace(' ', "-")
        );
    }
    let mut input = Input::<String>::new().with_prompt(prompt.to_string());
    if let Some(default) = default {
        input = input.default(default.to_string());
    }
    Ok(input.interact_text()?.trim().to_string())
}

fn optional_value(
    value: Option<String>,
    non_interactive: bool,
    prompt: &str,
    default: &str,
) -> Result<String> {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        return Ok(value.trim().to_string());
    }
    if non_interactive {
        return Ok(default.to_string());
    }
    Ok(Input::<String>::new()
        .with_prompt(prompt)
        .default(default.to_string())
        .interact_text()?
        .trim()
        .to_string())
}

fn select_clients(requested: Vec<ClientKind>, non_interactive: bool) -> Result<Vec<ClientKind>> {
    if !requested.is_empty() {
        return Ok(requested);
    }
    let detected = registration::detect_clients();
    if non_interactive {
        return Ok(detected);
    }
    let all = [ClientKind::Claude, ClientKind::Codex, ClientKind::Cursor];
    let labels = all.map(|client| client.label());
    let defaults = all.map(|client| detected.contains(&client));
    Ok(MultiSelect::new()
        .with_prompt("Agent clients to configure")
        .items(&labels)
        .defaults(&defaults)
        .interact()?
        .into_iter()
        .map(|index| all[index])
        .collect())
}

fn suggested_principal() -> Option<String> {
    std::env::var("HELIXIR_RBAC_ACTOR")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var("USER").ok())
        .map(|value| value.to_ascii_lowercase().replace(' ', "-"))
}

fn validate_principal(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 {
        bail!("principal must contain 1..=128 characters");
    }
    if !value.chars().all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || "-_.@".contains(character)
    }) {
        bail!(
            "principal may contain only lower-case ASCII letters, digits, '-', '_', '.', and '@'"
        );
    }
    Ok(())
}

fn validate_owner(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        bail!("owner must contain 1..=128 printable characters");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_validation_matches_server_contract() {
        for accepted in ["codex", "codex-laptop", "nikita@workstation"] {
            validate_principal(accepted).unwrap();
        }
        for rejected in ["", "Codex", "two words", "../escape", "кириллица"] {
            assert!(validate_principal(rejected).is_err());
        }
        validate_owner("Codex").unwrap();
        validate_owner("Никита").unwrap();
        assert!(validate_owner("line\nbreak").is_err());
    }
}
