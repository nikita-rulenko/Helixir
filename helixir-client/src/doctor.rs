//! Readiness checks for the remote gateway and every local client artifact.

use anyhow::{Result, bail};

use crate::gateway::McpClient;
use crate::instructions;
use crate::profile::ClientProfile;
use crate::registration;

pub(crate) const REQUIRED_TOOLS: &[&str] = &[
    "enroll_client",
    "add_memory",
    "get_add_status",
    "search_memory",
    "search_by_concept",
    "search_reasoning_chain",
    "connect_memories",
    "get_memory_graph",
    "update_memory",
    "list_memories",
    "list_users",
    "agent_heartbeat",
    "swarm_status",
    "resolve_contradiction",
    "agent_farewell",
    "think_start",
    "think_add",
    "think_recall",
    "think_conclude",
    "think_commit",
    "think_discard",
    "think_status",
    "search_incomplete_thoughts",
];

pub(crate) fn missing_required_tools(
    advertised: &std::collections::BTreeSet<String>,
) -> Vec<&'static str> {
    REQUIRED_TOOLS
        .iter()
        .filter(|tool| !advertised.contains(**tool))
        .copied()
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

pub fn inspect(profile: &ClientProfile) -> Vec<Check> {
    let mut checks = Vec::new();
    let token = token_from_env(&profile.token_env);
    let token_env = token.as_ref().map(|_| profile.token_env.as_str());
    match McpClient::connect(&profile.gateway_url, token) {
        Ok(mut gateway) => {
            match gateway.tool_names() {
                Ok(tools) => {
                    let missing = missing_required_tools(&tools);
                    checks.push(Check {
                        name: "gateway tools".to_string(),
                        ok: missing.is_empty(),
                        detail: if missing.is_empty() {
                            format!(
                                "gateway {} advertises {} required MCP tools",
                                gateway.server_version(),
                                tools.len()
                            )
                        } else {
                            format!("missing {}", missing.join(", "))
                        },
                    });
                }
                Err(error) => checks.push(failed("gateway tools", error)),
            }
            match gateway.enroll_client(&profile.principal_id) {
                Ok(enrollment) => checks.push(Check {
                    name: "RBAC enrollment".to_string(),
                    ok: enrollment.principal_id == profile.principal_id
                        && !enrollment.roles.is_empty(),
                    detail: format!(
                        "principal={} group={} roles={}",
                        enrollment.principal_id,
                        enrollment.group_id,
                        enrollment.roles.join(",")
                    ),
                }),
                Err(error) => checks.push(failed("RBAC enrollment", error)),
            }
        }
        Err(error) => checks.push(failed("gateway connection", error)),
    }
    for client in &profile.clients {
        match registration::registration_matches(*client, &profile.gateway_url, token_env) {
            Ok(matches) => checks.push(Check {
                name: format!("{} MCP registration", client.label()),
                ok: matches,
                detail: if matches {
                    profile.gateway_url.clone()
                } else {
                    "missing or points at another endpoint".to_string()
                },
            }),
            Err(error) => checks.push(failed(
                &format!("{} MCP registration", client.label()),
                error,
            )),
        }
    }
    match instructions::verify(profile) {
        Ok(failures) => checks.push(Check {
            name: "agent instructions".to_string(),
            ok: failures.is_empty(),
            detail: if failures.is_empty() {
                "canonical skill and managed AGENTS.md are current".to_string()
            } else {
                failures.join("; ")
            },
        }),
        Err(error) => checks.push(failed("agent instructions", error)),
    }
    checks
}

pub fn run(profile: &ClientProfile) -> Result<()> {
    let checks = inspect(profile);
    for check in &checks {
        println!(
            "{} {:<24} {}",
            if check.ok { "OK" } else { "FAIL" },
            check.name,
            check.detail
        );
    }
    if checks.iter().any(|check| !check.ok) {
        bail!("one or more helixir-client checks failed");
    }
    Ok(())
}

pub fn token_from_env(name: &str) -> Option<String> {
    (!name.is_empty())
        .then(|| std::env::var(name).ok())
        .flatten()
        .filter(|value| !value.is_empty())
}

fn failed(name: &str, error: impl std::fmt::Display) -> Check {
    Check {
        name: name.to_string(),
        ok: false,
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{REQUIRED_TOOLS, missing_required_tools};

    #[test]
    fn client_contract_rejects_a_gateway_missing_lifecycle_or_fastthink() {
        let complete = REQUIRED_TOOLS
            .iter()
            .map(|tool| (*tool).to_string())
            .collect::<BTreeSet<_>>();
        assert!(missing_required_tools(&complete).is_empty());

        let mut partial = complete;
        partial.remove("agent_farewell");
        partial.remove("think_commit");
        assert_eq!(
            missing_required_tools(&partial),
            vec!["agent_farewell", "think_commit"]
        );
    }
}
