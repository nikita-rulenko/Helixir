//! Pure in-memory RBAC policy evaluation.

use super::*;

impl RbacPolicy {
    pub fn group(&self, group: &str) -> Result<&Group> {
        self.groups
            .get(group)
            .ok_or_else(|| anyhow::anyhow!("unknown RBAC group '{group}'"))
    }

    pub fn upsert_group(&mut self, id: impl Into<String>, description: impl Into<String>) {
        let id = id.into();
        self.groups.insert(
            id.clone(),
            Group {
                name: id,
                description: description.into(),
                dedup_group_id: None,
            },
        );
    }

    pub fn remove_group(&mut self, group: &str) -> bool {
        let removed = self.groups.remove(group).is_some();
        for binding in self.users.values_mut() {
            binding.groups.remove(group);
        }
        removed
    }

    pub fn upsert_dedup_group(&mut self, id: impl Into<String>, description: impl Into<String>) {
        let id = id.into();
        self.dedup_groups.insert(
            id.clone(),
            DedupGroup {
                name: id,
                description: description.into(),
            },
        );
    }

    pub fn assign_dedup_group(&mut self, group: &str, dedup_group: Option<&str>) -> Result<()> {
        if let Some(id) = dedup_group
            && !self.dedup_groups.contains_key(id)
        {
            bail!("unknown RBAC dedup group '{id}'");
        }
        self.group(group)?;
        if let Some(entry) = self.groups.get_mut(group) {
            entry.dedup_group_id = dedup_group.map(str::to_string);
        }
        Ok(())
    }

    pub fn resolve_memory_scope(&self, group_id: Option<&str>) -> Result<RbacMemoryScope> {
        if !self.enabled {
            return Ok(RbacMemoryScope::Legacy);
        }
        let Some(group_id) = group_id else {
            return Ok(RbacMemoryScope::Unscoped);
        };
        let group = self.group(group_id)?;
        let Some(dedup_group_id) = group.dedup_group_id.as_deref() else {
            if group_id == crate::core::rbac_compat::ONBOARDING_GROUP_ID {
                return Ok(RbacMemoryScope::CompatibilityGroup {
                    group_id: group_id.to_string(),
                });
            }
            return Ok(RbacMemoryScope::Group {
                group_id: group_id.to_string(),
            });
        };
        let group_ids = self
            .groups
            .iter()
            .filter(|(_, group)| group.dedup_group_id.as_deref() == Some(dedup_group_id))
            .map(|(id, _)| id.clone())
            .collect();
        Ok(RbacMemoryScope::DedupGroup {
            dedup_group_id: dedup_group_id.to_string(),
            group_ids,
        })
    }

    pub fn assign_global(&mut self, user: &str, role: Role) {
        self.users
            .entry(user.to_string())
            .or_default()
            .global_roles
            .insert(role);
    }

    pub fn assign_group(&mut self, user: &str, group: &str, role: Role) -> Result<()> {
        self.group(group)?;
        self.users
            .entry(user.to_string())
            .or_default()
            .groups
            .entry(group.to_string())
            .or_default()
            .insert(role);
        Ok(())
    }

    pub fn revoke_global(&mut self, user: &str, role: Role) -> bool {
        self.users
            .get_mut(user)
            .is_some_and(|binding| binding.global_roles.remove(&role))
    }

    pub fn revoke_group(&mut self, user: &str, group: &str, role: Role) -> bool {
        let Some(binding) = self.users.get_mut(user) else {
            return false;
        };
        let Some(roles) = binding.groups.get_mut(group) else {
            return false;
        };
        let removed = roles.remove(&role);
        if roles.is_empty() {
            binding.groups.remove(group);
        }
        removed
    }

    pub fn roles_for(&self, user: &str) -> Vec<(Option<&str>, Role)> {
        let Some(binding) = self.users.get(user) else {
            return Vec::new();
        };
        let mut out = binding
            .global_roles
            .iter()
            .copied()
            .map(|role| (None, role))
            .collect::<Vec<_>>();
        for (group, roles) in &binding.groups {
            out.extend(
                roles
                    .iter()
                    .copied()
                    .map(|role| (Some(group.as_str()), role)),
            );
        }
        out
    }

    pub fn is_admin(&self, user: &str) -> bool {
        self.users
            .get(user)
            .is_some_and(|binding| binding.global_roles.contains(&Role::Admin))
    }

    /// Buffered write results are private operational data. The owner, the
    /// principal that created the write, and global admins may inspect them;
    /// ordinary group visibility is intentionally insufficient.
    pub fn can_read_pending(&self, actor: &str, owner: &str, creator: &str) -> bool {
        !self.enabled || self.is_admin(actor) || actor == owner || actor == creator
    }

    /// Outbox notices may contain failed raw inputs and charter escalations,
    /// so they are private to the owner (plus global administrators).
    pub fn can_read_outbox(&self, actor: &str, owner: &str) -> bool {
        !self.enabled || self.is_admin(actor) || actor == owner
    }

    /// Groups whose memories the actor can read.
    pub fn readable_groups(&self, actor: &str) -> Option<HashSet<String>> {
        if !self.enabled {
            return None;
        }
        let Some(binding) = self.users.get(actor) else {
            return Some(HashSet::new());
        };
        if binding.global_roles.contains(&Role::Admin) {
            return None;
        }
        Some(
            binding
                .groups
                .iter()
                .filter(|(_, roles)| roles.iter().any(|role| role.can_read()))
                .map(|(group, _)| group.clone())
                .collect(),
        )
    }

    /// Return the user ids whose memories are visible to `actor`.
    /// `None` means unrestricted (global admin); an empty set means deny.
    pub fn readable_users(&self, actor: &str) -> Option<HashSet<String>> {
        let groups = self.readable_groups(actor)?;
        let mut users = HashSet::new();
        if self
            .users
            .get(actor)
            .is_some_and(|binding| !binding.global_roles.is_empty() || !binding.groups.is_empty())
        {
            users.insert(actor.to_string());
        }
        for (user, binding) in &self.users {
            if binding.groups.keys().any(|group| groups.contains(group)) {
                users.insert(user.clone());
            }
        }
        Some(users)
    }

    pub fn can_write(&self, actor: &str) -> bool {
        if !self.enabled {
            return true;
        }
        let Some(binding) = self.users.get(actor) else {
            return false;
        };
        binding.global_roles.iter().any(|role| role.can_write())
            || binding
                .groups
                .values()
                .flatten()
                .any(|role| role.can_write())
    }

    pub fn can_write_owner(&self, actor: &str, owner: &str) -> bool {
        if !self.enabled {
            return true;
        }
        if self.is_admin(actor) {
            return true;
        }
        if actor == owner && self.can_write(actor) {
            return true;
        }
        let Some(actor_groups) = self.readable_groups(actor) else {
            return true;
        };
        let owner_groups = self
            .users
            .get(owner)
            .map(|binding| binding.groups.keys().collect::<HashSet<_>>())
            .unwrap_or_default();
        self.users.get(actor).is_some_and(|binding| {
            binding.groups.iter().any(|(group, roles)| {
                actor_groups.contains(group)
                    && owner_groups.contains(group)
                    && roles
                        .iter()
                        .any(|role| matches!(role, Role::GroupAdmin | Role::Moderator))
            })
        })
    }

    /// Whether an actor may create a new memory owned by `owner`.
    /// Global administrators can write for anyone; group administrators and
    /// moderators can write for members of their groups; workers can only
    /// create memories under their own identity.
    pub fn can_create_for(&self, actor: &str, owner: &str) -> bool {
        if !self.enabled {
            return true;
        }
        if actor == owner {
            return self.can_write(actor);
        }
        if self.is_admin(actor) {
            return true;
        }
        let Some(actor_binding) = self.users.get(actor) else {
            return false;
        };
        let Some(owner_binding) = self.users.get(owner) else {
            return false;
        };
        actor_binding.groups.iter().any(|(group, roles)| {
            owner_binding.groups.contains_key(group)
                && roles
                    .iter()
                    .any(|role| matches!(role, Role::GroupAdmin | Role::Moderator))
        })
    }

    /// Whether `actor` may create a memory owned by `owner` in one explicit
    /// group. The onboarding profile may infer its reserved enrollment group
    /// before calling this method.
    pub fn can_create_for_group(&self, actor: &str, owner: &str, group: Option<&str>) -> bool {
        if !self.enabled {
            return true;
        }
        if self.is_admin(actor) {
            return group.is_none() || group.is_some_and(|id| self.groups.contains_key(id));
        }
        let Some(group) = group else {
            return false;
        };
        if !self.groups.contains_key(group) {
            return false;
        }
        let Some(actor_binding) = self.users.get(actor) else {
            return false;
        };
        let Some(actor_roles) = actor_binding.groups.get(group) else {
            return false;
        };
        if actor == owner {
            return actor_roles.iter().any(|role| role.can_write());
        }
        if group == crate::core::rbac_compat::ONBOARDING_GROUP_ID
            && actor_roles
                .iter()
                .any(|role| matches!(role, Role::GroupAdmin | Role::Moderator))
        {
            return true;
        }
        let owner_is_member = self
            .users
            .get(owner)
            .is_some_and(|binding| binding.groups.contains_key(group));
        owner_is_member
            && actor_roles
                .iter()
                .any(|role| matches!(role, Role::GroupAdmin | Role::Moderator))
    }

    /// Whether `actor` may mutate an existing memory with the supplied group
    /// edges. An unscoped memory is admin-only while RBAC is enabled.
    pub fn can_write_memory(
        &self,
        actor: &str,
        owner: &str,
        memory_groups: &HashSet<String>,
    ) -> bool {
        if !self.enabled || self.is_admin(actor) {
            return true;
        }
        let Some(actor_binding) = self.users.get(actor) else {
            return false;
        };
        if actor == owner {
            return memory_groups.iter().any(|group| {
                actor_binding
                    .groups
                    .get(group)
                    .is_some_and(|roles| roles.iter().any(|role| role.can_write()))
            });
        }
        if memory_groups.contains(crate::core::rbac_compat::ONBOARDING_GROUP_ID)
            && actor_binding
                .groups
                .get(crate::core::rbac_compat::ONBOARDING_GROUP_ID)
                .is_some_and(|roles| {
                    roles
                        .iter()
                        .any(|role| matches!(role, Role::GroupAdmin | Role::Moderator))
                })
        {
            return true;
        }
        let Some(owner_binding) = self.users.get(owner) else {
            return false;
        };
        memory_groups.iter().any(|group| {
            owner_binding.groups.contains_key(group)
                && actor_binding.groups.get(group).is_some_and(|roles| {
                    roles
                        .iter()
                        .any(|role| matches!(role, Role::GroupAdmin | Role::Moderator))
                })
        })
    }

    /// Legacy owner-level filtering used only by disabled-mode compatibility
    /// callers. Enabled read paths must use `RbacManager::visible_memory_ids`
    /// so authorization is based on per-memory group edges.
    pub fn filter_results<T>(
        &self,
        actor: &str,
        results: Vec<T>,
        owner: impl Fn(&T) -> Option<&str>,
    ) -> Vec<T> {
        let Some(allowed) = self.readable_users(actor) else {
            return results;
        };
        results
            .into_iter()
            .filter(|row| owner(row).is_some_and(|user| allowed.contains(user)))
            .collect()
    }

    pub fn validate(&self) -> Result<()> {
        for (user, binding) in &self.users {
            if user.trim().is_empty() {
                bail!("RBAC user id cannot be empty");
            }
            for group in binding.groups.keys() {
                self.group(group)?;
            }
        }
        for group in self.groups.values() {
            if let Some(dedup_group) = group.dedup_group_id.as_deref()
                && !self.dedup_groups.contains_key(dedup_group)
            {
                bail!("unknown RBAC dedup group '{dedup_group}'");
            }
        }
        Ok(())
    }
}
