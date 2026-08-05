//! Role-based access control for Helixir memory operations.
//!
//! A missing `RbacConfig` row (or `enabled = 0`) exists only during the
//! resumable one-way bootstrap. In the permanent active state,
//! the policy maps users to global roles and group-scoped roles. Memory rows
//! remain owned by their existing `user_id`; strict visibility is derived from
//! explicit `MEMORY_IN_RBAC_GROUP` edges, so authorship is not overloaded and
//! a multi-group owner cannot accidentally share one memory with every group.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::db::HelixClient;

mod manager_admin;
mod manager_authorization;
mod manager_cache;
mod manager_memory;
mod policy;
mod storage_types;
mod types;

pub use manager_admin::RbacManager;
use manager_cache::*;
use storage_types::*;
pub use types::*;

#[cfg(test)]
#[path = "rbac_tests.rs"]
mod tests;
