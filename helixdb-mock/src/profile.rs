//! Deterministic workload profiles.

use crate::registry::{QuerySpec, ReturnKind};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Latency and fixture-density profile.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    /// Small fixtures and near-zero latency for contract tests.
    #[default]
    Fast,
    /// Approximation of measured v2.3.5 response classes.
    RecordedV235,
    /// Bounded large fixtures and elevated latency for pressure tests.
    Stress,
}

/// Deterministic state family used independently from latency.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Scenario {
    /// No seeded rows; useful for missing-object semantics.
    BootstrapEmpty,
    /// Coherent redacted skeleton plus the measured aggregate census.
    #[default]
    #[value(name = "baseline-5k")]
    #[serde(rename = "baseline-5k")]
    Baseline5k,
    /// Dense category-list responses for daemon sampling.
    DaemonDenseCategory,
    /// Multiple synthetic RBAC groups and assignments.
    RbacMultiGroup,
    /// Pending ingest and delivery-notice records.
    IngestQueue,
    /// Dense reasoning-edge responses.
    ReasoningDense,
    /// Five hundred fully scoped memories for the Atropos candidate budget gate.
    Merge500,
    /// Deterministic non-200 fault injection.
    Errors,
}

impl Profile {
    /// Deterministic row count, capped by a request's explicit `limit`.
    #[must_use]
    pub fn row_count(self, requested_limit: Option<usize>) -> usize {
        let base = match self {
            Self::Fast => 1,
            Self::RecordedV235 => 6,
            Self::Stress => 64,
        };
        requested_limit.map_or(base, |limit| base.min(limit))
    }

    /// Deterministic latency based on query class and a stable hash.
    #[must_use]
    pub fn latency(self, query: &QuerySpec, seed: u64, request_hash: u64) -> Duration {
        let is_list = query
            .returns
            .iter()
            .any(|field| field.kind == ReturnKind::Array);
        let (base, spread) = match (self, query.is_vector(), query.mutation, is_list) {
            (Self::Fast, _, _, true) => (1, 5),
            (Self::Fast, _, _, false) => (0, 3),
            (Self::RecordedV235, true, _, _) => (28, 19),
            (Self::RecordedV235, false, true, _) => (12, 11),
            (Self::RecordedV235, false, false, true) => (9, 12),
            (Self::RecordedV235, false, false, false) => (7, 9),
            (Self::Stress, true, _, _) => (140, 121),
            (Self::Stress, false, true, _) => (85, 83),
            (Self::Stress, false, false, true) => (70, 79),
            (Self::Stress, false, false, false) => (55, 67),
        };
        let jitter = (request_hash ^ seed ^ stable_name_hash(query.name)) % spread;
        Duration::from_millis(base + jitter)
    }

    /// Aggregate-only live census used by the v2.3.5 differential baseline.
    /// No production content is embedded in the emulator.
    #[must_use]
    pub fn baseline_count(self, label: &str) -> usize {
        if self == Self::Fast {
            return 0;
        }
        let value: usize = match label {
            "memory" => 5_883,
            "user" => 676,
            "entity" => 4_149,
            "concept" => 20,
            "category" => 162,
            "agent" | "rbac_group" | "group" => 114,
            "rbac_dedup_group" | "dedup_group" => 27,
            "rbac_assignment" | "assignment" => 1_527,
            "context" => 4_119,
            "memory_chunk" => 167,
            "tagged_as" => 24_417,
            "memory_relation" => 20_746,
            "extracted_entity" => 9_307,
            "instance_of" => 7_154,
            "has_embedding" => 5_887,
            "has_memory" => 5_890,
            "has_history" => 5_826,
            "memory_in_rbac_group" => 5_813,
            "contradicts" => 2_838,
            "because" => 914,
            "implies" => 241,
            "moirai_derived_from" => 248,
            _ => 0,
        };
        if self == Self::Stress {
            value.saturating_mul(2)
        } else {
            value
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::RecordedV235 => "recorded-v235",
            Self::Stress => "stress",
        }
    }
}

impl Scenario {
    /// Return the bounded synthetic row count for a query.
    #[must_use]
    pub fn row_count(self, query: &str, profile: Profile, limit: Option<usize>) -> usize {
        let base = profile.row_count(limit);
        let desired = match self {
            Self::BootstrapEmpty | Self::Errors => 0,
            Self::Merge500 if query == "getRecentMemories" => 500,
            Self::Merge500 if query == "getMemoryRbacScopesBatch" => 500,
            Self::Merge500 if query == "smartVectorSearchWithChunks" => 25,
            Self::DaemonDenseCategory if query.contains("Category") => base.max(48),
            Self::RbacMultiGroup if query.contains("Rbac") => base.max(12),
            Self::IngestQueue if query.contains("Pending") || query.contains("Notice") => {
                base.max(32)
            }
            Self::ReasoningDense
                if query.contains("Relation")
                    || query.contains("Connection")
                    || query.contains("Reasoning") =>
            {
                base.max(48)
            }
            _ => base,
        };
        limit.map_or(desired, |limit| desired.min(limit))
    }

    /// Return a redacted aggregate count for the chosen fixture family.
    #[must_use]
    pub fn census_count(self, profile: Profile, label: &str) -> usize {
        match self {
            Self::BootstrapEmpty | Self::Errors => 0,
            _ => profile.baseline_count(label),
        }
    }

    /// Whether every data query should return a deterministic failure.
    #[must_use]
    pub const fn inject_error(self) -> bool {
        matches!(self, Self::Errors)
    }

    /// Stable CLI/trace name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BootstrapEmpty => "bootstrap-empty",
            Self::Baseline5k => "baseline-5k",
            Self::DaemonDenseCategory => "daemon-dense-category",
            Self::RbacMultiGroup => "rbac-multi-group",
            Self::IngestQueue => "ingest-queue",
            Self::ReasoningDense => "reasoning-dense",
            Self::Merge500 => "merge-500",
            Self::Errors => "errors",
        }
    }
}

fn stable_name_hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::find_query;

    #[test]
    fn latency_is_reproducible_and_vector_queries_are_slower() {
        let vector = find_query("globalVectorSearch").unwrap();
        let scalar = find_query("countAllMemories").unwrap();
        let first = Profile::RecordedV235.latency(vector, 7, 11);
        assert_eq!(first, Profile::RecordedV235.latency(vector, 7, 11));
        assert!(first > Profile::RecordedV235.latency(scalar, 7, 11));
    }

    #[test]
    fn requested_limit_caps_rows() {
        assert_eq!(Profile::Stress.row_count(Some(3)), 3);
        assert_eq!(Profile::Stress.row_count(None), 64);
    }

    #[test]
    fn recorded_profile_uses_redacted_live_census() {
        assert_eq!(Profile::RecordedV235.baseline_count("memory"), 5_883);
        assert_eq!(Profile::RecordedV235.baseline_count("tagged_as"), 24_417);
        assert_eq!(Profile::Fast.baseline_count("memory"), 0);
    }

    #[test]
    fn scenarios_are_bounded_by_caller_limit() {
        assert_eq!(
            Scenario::DaemonDenseCategory.row_count("getAllCategories", Profile::Stress, Some(7)),
            7
        );
        assert_eq!(
            Scenario::BootstrapEmpty.row_count("getAllCategories", Profile::Stress, None),
            0
        );
        assert_eq!(
            Scenario::Merge500.row_count("getRecentMemories", Profile::Fast, Some(500)),
            500
        );
        assert_eq!(
            Scenario::Merge500.row_count("smartVectorSearchWithChunks", Profile::Fast, Some(25)),
            25
        );
    }
}
