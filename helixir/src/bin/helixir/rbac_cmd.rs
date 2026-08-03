use super::*;

pub(crate) fn parse_rbac_role(raw: &str) -> Result<Role> {
    Role::parse(raw).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown RBAC role '{raw}'; use admin, teamlead, groupadmin, moderator, worker, viewer"
        )
    })
}

pub(crate) fn rbac_actor() -> String {
    std::env::var("HELIXIR_RBAC_ACTOR").unwrap_or_else(|_| "cli".to_string())
}

pub(crate) async fn privileged(
    client: &HelixirClient,
) -> Result<helixir::core::helixir_client::HelixirAdmin<'_>> {
    let actor = rbac_actor();
    client
        .admin_as(&actor)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))
}

pub(crate) fn require_rbac_admin(
    policy: &helixir::core::rbac::RbacPolicy,
    actor: &str,
    action: &str,
) -> Result<()> {
    if policy.enabled && !policy.is_admin(actor) {
        anyhow::bail!("{action} requires a global admin (set HELIXIR_RBAC_ACTOR)");
    }
    Ok(())
}

pub(crate) async fn rbac_run(client: &HelixirClient, cmd: RbacCmd) -> Result<()> {
    let manager = client.rbac();
    let current = manager
        .snapshot()
        .await
        .context("read RBAC state from HelixDB")?;
    match cmd {
        RbacCmd::Status { json } => {
            if json {
                let actor = rbac_actor();
                require_rbac_admin(&current, &actor, "RBAC status inspection")?;
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&current)?);
            } else {
                println!(
                    "RBAC: {}",
                    if current.enabled {
                        "enabled"
                    } else {
                        "disabled (full trust)"
                    }
                );
                println!(
                    "groups: {}  principals: {}",
                    current.groups.len(),
                    current.users.len()
                );
            }
        }
        RbacCmd::Enable | RbacCmd::Disable => {
            let enabled = matches!(cmd, RbacCmd::Enable);
            let actor = rbac_actor();
            require_rbac_admin(&current, &actor, "RBAC management")?;
            manager.set_enabled(enabled, &actor).await?;
            println!("RBAC {}", if enabled { "enabled" } else { "disabled" });
        }
        RbacCmd::Group { cmd } => match cmd {
            RbacGroupCmd::Create {
                id,
                name,
                description,
            } => {
                let actor = rbac_actor();
                require_rbac_admin(&current, &actor, "group management")?;
                manager
                    .create_group_as(&id, &name, &description, &actor)
                    .await?;
                println!("group '{id}' created/updated");
            }
            RbacGroupCmd::List { json } => {
                let actor = rbac_actor();
                require_rbac_admin(&current, &actor, "group listing")?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&current.groups)?);
                } else {
                    for (id, group) in current.groups {
                        println!("{id}\t{}\t{}", group.name, group.description);
                    }
                }
            }
            RbacGroupCmd::Delete { id, yes } => {
                anyhow::ensure!(yes, "deactivating a group requires --yes");
                let actor = rbac_actor();
                require_rbac_admin(&current, &actor, "group management")?;
                manager.deactivate_group_as(&id, &actor).await?;
                println!("group '{id}' deactivated");
            }
        },
        RbacCmd::Dedup { cmd } => {
            let actor = rbac_actor();
            require_rbac_admin(&current, &actor, "dedup group management")?;
            match cmd {
                RbacDedupCmd::Create {
                    id,
                    name,
                    description,
                } => {
                    manager
                        .create_dedup_group_as(&id, &name, &description, &actor)
                        .await?;
                    println!("dedup group '{id}' created/updated");
                }
                RbacDedupCmd::List { json } => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "dedup_groups": current.dedup_groups,
                                "groups": current.groups,
                            }))?
                        );
                    } else {
                        for (id, dedup) in &current.dedup_groups {
                            let members = current
                                .groups
                                .iter()
                                .filter(|(_, group)| {
                                    group.dedup_group_id.as_deref() == Some(id.as_str())
                                })
                                .map(|(group_id, _)| group_id.as_str())
                                .collect::<Vec<_>>()
                                .join(",");
                            println!("{id}\t{}\t{members}", dedup.name);
                        }
                    }
                }
                RbacDedupCmd::Attach { group, dedup_group } => {
                    let backfilled = manager
                        .attach_group_to_dedup_as(&group, &dedup_group, &actor)
                        .await?;
                    println!(
                        "group '{group}' attached to dedup group '{dedup_group}' ({backfilled} historical memories linked)"
                    );
                }
                RbacDedupCmd::Detach { group } => {
                    manager.detach_group_from_dedup_as(&group, &actor).await?;
                    println!("group '{group}' detached; historical access retained");
                }
                RbacDedupCmd::Delete { id, yes } => {
                    anyhow::ensure!(yes, "deactivating a dedup group requires --yes");
                    manager.deactivate_dedup_group_as(&id, &actor).await?;
                    println!("dedup group '{id}' deactivated");
                }
            }
        }
        RbacCmd::Grant { user, role, group } => {
            let role = parse_rbac_role(&role)?;
            let actor = rbac_actor();
            require_rbac_admin(&current, &actor, "grant")?;
            manager.grant(&user, role, group.as_deref(), &actor).await?;
            println!(
                "granted {} to {}{}",
                role.label(),
                user,
                group.map(|g| format!(" in {g}")).unwrap_or_default()
            );
        }
        RbacCmd::Revoke { user, role, group } => {
            let role = parse_rbac_role(&role)?;
            let actor = rbac_actor();
            require_rbac_admin(&current, &actor, "revoke")?;
            manager
                .revoke_as(&user, role, group.as_deref(), &actor)
                .await?;
            println!("revoked {} from {}", role.label(), user);
        }
        RbacCmd::Show { user, json: _ } => {
            let actor = rbac_actor();
            if user.as_deref() != Some(actor.as_str()) {
                require_rbac_admin(&current, &actor, "role inspection")?;
            }
            let rows: serde_json::Value = user
                .as_deref()
                .map(|id| serde_json::json!({"user": id, "roles": current.roles_for(id).into_iter().map(|(group, role)| serde_json::json!({"group": group, "role": role.label()})).collect::<Vec<_>>() }))
                .unwrap_or_else(|| serde_json::to_value(&current.users).unwrap_or_default());
            println!("{}", serde_json::to_string_pretty(&rows)?);
        }
        RbacCmd::Check {
            user,
            action,
            owner,
        } => {
            let actor = rbac_actor();
            if current.enabled && actor != user && !current.is_admin(&actor) {
                anyhow::bail!("RBAC check for another principal requires a global admin");
            }
            let allowed = match action.as_str() {
                "read" => current.readable_users(&user).is_none_or(|users| {
                    owner.as_deref().is_none_or(|target| users.contains(target))
                }),
                "write" => owner
                    .as_deref()
                    .map(|target| current.can_write_owner(&user, target))
                    .unwrap_or_else(|| current.can_write(&user)),
                other => anyhow::bail!("unknown action '{other}'; use read or write"),
            };
            println!("{}", if allowed { "allowed" } else { "denied" });
            if !allowed {
                anyhow::bail!("RBAC denied");
            }
        }
    }
    Ok(())
}
