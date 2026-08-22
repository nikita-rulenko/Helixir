//! Canonical lifecycle inventory for every physical HelixDB declaration.
//!
//! The HQL schema remains the persistence contract. This module adds the
//! product-lifecycle contract: whether each declaration is live, intentionally
//! reserved, or deprecated, plus the evidence required for that status.

mod edges;
mod nodes;

use std::collections::BTreeMap;

use serde::Serialize;

use crate::db::HelixClient;

pub use edges::EDGES;
pub use nodes::{NODES, VECTORS};

/// Physical family used by HelixDB's typed schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SchemaKind {
    Node,
    Vector,
    Edge,
}

impl SchemaKind {
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Node => "N",
            Self::Vector => "V",
            Self::Edge => "E",
        }
    }
}

/// Product lifecycle of a physical schema declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SchemaLifecycle {
    Active,
    Reserved,
    Deprecated,
}

/// Machine-checked declaration metadata.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct SchemaDeclaration {
    pub kind: SchemaKind,
    pub name: &'static str,
    pub lifecycle: SchemaLifecycle,
    pub owner: &'static str,
    pub milestone: Option<&'static str>,
    pub producer: Option<&'static str>,
    pub consumer: Option<&'static str>,
    pub e2e: Option<&'static str>,
    pub migration: Option<&'static str>,
    pub purpose: &'static str,
    pub count_key: &'static str,
}

impl SchemaDeclaration {
    #[expect(
        clippy::too_many_arguments,
        reason = "the const constructor mirrors one declarative inventory row; grouping these scalar evidence fields would make the static ledger less auditable"
    )]
    pub const fn active(
        kind: SchemaKind,
        name: &'static str,
        owner: &'static str,
        producer: &'static str,
        consumer: &'static str,
        e2e: &'static str,
        purpose: &'static str,
        count_key: &'static str,
    ) -> Self {
        Self {
            kind,
            name,
            lifecycle: SchemaLifecycle::Active,
            owner,
            milestone: None,
            producer: Some(producer),
            consumer: Some(consumer),
            e2e: Some(e2e),
            migration: None,
            purpose,
            count_key,
        }
    }

    pub const fn reserved(
        kind: SchemaKind,
        name: &'static str,
        owner: &'static str,
        milestone: &'static str,
        purpose: &'static str,
        count_key: &'static str,
    ) -> Self {
        Self {
            kind,
            name,
            lifecycle: SchemaLifecycle::Reserved,
            owner,
            milestone: Some(milestone),
            producer: None,
            consumer: None,
            e2e: None,
            migration: None,
            purpose,
            count_key,
        }
    }

    pub const fn deprecated(
        kind: SchemaKind,
        name: &'static str,
        owner: &'static str,
        migration: &'static str,
        purpose: &'static str,
        count_key: &'static str,
    ) -> Self {
        Self {
            kind,
            name,
            lifecycle: SchemaLifecycle::Deprecated,
            owner,
            milestone: None,
            producer: None,
            consumer: None,
            e2e: None,
            migration: Some(migration),
            purpose,
            count_key,
        }
    }

    pub fn physical_name(self) -> String {
        format!("{}::{}", self.kind.prefix(), self.name)
    }
}

/// One declaration paired with its server-side count, when the census query
/// was available in the deployed schema.
#[derive(Debug, Clone, Serialize)]
pub struct SchemaCensusItem {
    #[serde(flatten)]
    pub declaration: SchemaDeclaration,
    pub count: Option<u64>,
}

/// Bounded production census used by the admin API and release verification.
#[derive(Debug, Clone, Serialize)]
pub struct SchemaInventoryReport {
    pub inventory_version: u32,
    pub items: Vec<SchemaCensusItem>,
    pub active: usize,
    pub reserved: usize,
    pub deprecated: usize,
    pub counted: usize,
    pub failed_queries: Vec<&'static str>,
}

/// Complete versioned inventory in physical schema order.
pub fn declarations() -> impl Iterator<Item = SchemaDeclaration> {
    NODES
        .iter()
        .chain(VECTORS.iter())
        .chain(EDGES.iter())
        .copied()
}

/// Execute three server-side aggregate queries. No node, vector or edge rows
/// cross the wire; only the named counts do.
pub async fn census(db: &HelixClient) -> SchemaInventoryReport {
    let mut counts = BTreeMap::new();
    let mut failed_queries = Vec::new();
    for query in [
        "getSchemaNodeCensus",
        "getSchemaVectorCensus",
        "getSchemaEdgeCensus",
    ] {
        match db
            .execute_query_no_retry::<serde_json::Value, _>(query, &serde_json::json!({}))
            .await
        {
            Ok(value) => collect_named_counts(&value, &mut counts),
            Err(_) => failed_queries.push(query),
        }
    }
    let items = declarations()
        .map(|declaration| SchemaCensusItem {
            count: counts.get(declaration.count_key).copied(),
            declaration,
        })
        .collect::<Vec<_>>();
    SchemaInventoryReport {
        inventory_version: 1,
        active: items
            .iter()
            .filter(|item| item.declaration.lifecycle == SchemaLifecycle::Active)
            .count(),
        reserved: items
            .iter()
            .filter(|item| item.declaration.lifecycle == SchemaLifecycle::Reserved)
            .count(),
        deprecated: items
            .iter()
            .filter(|item| item.declaration.lifecycle == SchemaLifecycle::Deprecated)
            .count(),
        counted: items.iter().filter(|item| item.count.is_some()).count(),
        failed_queries,
        items,
    }
}

fn collect_named_counts(value: &serde_json::Value, counts: &mut BTreeMap<String, u64>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if key.ends_with("_count")
                    && let Some(count) = first_unsigned(value)
                {
                    counts.insert(key.clone(), count);
                }
                collect_named_counts(value, counts);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_named_counts(value, counts);
            }
        }
        _ => {}
    }
}

fn first_unsigned(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(number) => number
            .as_u64()
            .or_else(|| number.as_i64().and_then(|value| u64::try_from(value).ok())),
        serde_json::Value::Array(values) => values.iter().find_map(first_unsigned),
        serde_json::Value::Object(values) => values.values().find_map(first_unsigned),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
