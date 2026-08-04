//! Public RBAC roles, groups, policy snapshots, and authorization decisions.

use super::*;

/// A role understood by the RBAC policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Global administrator: unrestricted read/write access.
    Admin,
    /// Team lead: read access to explicitly assigned groups.
    TeamLead,
    /// Group administrator: unrestricted access inside assigned groups.
    GroupAdmin,
    /// Group moderator: read/write access to assigned groups.
    Moderator,
    /// Worker (employee or agent): read/write own authored memories in group.
    Worker,
    /// Viewer: read-only access to assigned groups.
    Viewer,
}

impl Role {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "admin" | "administrator" => Some(Self::Admin),
            "teamlead" | "team-lead" | "team_lead" | "lead" => Some(Self::TeamLead),
            "groupadmin" | "group-admin" | "group_admin" => Some(Self::GroupAdmin),
            "moderator" | "mod" => Some(Self::Moderator),
            "worker" | "member" => Some(Self::Worker),
            "viewer" | "read-only" | "readonly" => Some(Self::Viewer),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::TeamLead => "teamlead",
            Self::GroupAdmin => "groupadmin",
            Self::Moderator => "moderator",
            Self::Worker => "worker",
            Self::Viewer => "viewer",
        }
    }

    pub(super) fn can_write(self) -> bool {
        !matches!(self, Self::Viewer | Self::TeamLead)
    }

    pub(super) fn can_read(self) -> bool {
        true
    }
}

/// A named group.  The identifier is stable and is used by CLI scripts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_group_id: Option<String>,
}

/// A stable federation of RBAC groups that intentionally shares deduplication
/// and visibility for memories created while those groups are members.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DedupGroup {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// Security domain resolved for one memory write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RbacMemoryScope {
    /// Historical full-trust behavior: global dedup and no RBAC edges.
    Legacy,
    /// Enabled-mode global-admin write with no group visibility.
    Unscoped,
    /// Default single-group profile: RBAC visibility with legacy-global
    /// fingerprints so an upgraded store keeps deduplicating existing rows.
    CompatibilityGroup { group_id: String },
    /// Private deduplication and visibility inside one concrete group.
    Group { group_id: String },
    /// Federated deduplication with materialized visibility for the current
    /// member groups.
    DedupGroup {
        dedup_group_id: String,
        group_ids: BTreeSet<String>,
    },
}

impl RbacMemoryScope {
    /// Stable salt for the content fingerprint. `None` preserves byte-for-byte
    /// legacy keys while every enabled RBAC domain gets an isolated namespace.
    pub fn fingerprint_scope(&self) -> Option<String> {
        match self {
            Self::Legacy => None,
            Self::Unscoped => Some("rbac:unscoped".to_string()),
            Self::CompatibilityGroup { .. } => None,
            Self::Group { group_id } => Some(format!("rbac:group:{group_id}")),
            Self::DedupGroup { dedup_group_id, .. } => Some(format!("rbac:dedup:{dedup_group_id}")),
        }
    }

    pub fn group_ids(&self) -> BTreeSet<String> {
        match self {
            Self::Group { group_id } | Self::CompatibilityGroup { group_id } => {
                BTreeSet::from([group_id.clone()])
            }
            Self::DedupGroup { group_ids, .. } => group_ids.clone(),
            Self::Legacy | Self::Unscoped => BTreeSet::new(),
        }
    }
}

/// Roles assigned to one user.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserBinding {
    #[serde(default)]
    pub global_roles: BTreeSet<Role>,
    #[serde(default)]
    pub groups: BTreeMap<String, BTreeSet<Role>>,
}

/// Persisted RBAC document.  Keep this format boring and hand-editable: it is
/// also the audit/debug surface used by `helixir rbac export`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RbacPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub groups: BTreeMap<String, Group>,
    #[serde(default)]
    pub dedup_groups: BTreeMap<String, DedupGroup>,
    #[serde(default)]
    pub users: BTreeMap<String, UserBinding>,
}
