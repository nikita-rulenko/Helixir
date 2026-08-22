use std::collections::BTreeSet;
use std::path::Path;

use super::*;

fn declared_in_hql() -> BTreeSet<(SchemaKind, String)> {
    include_str!("../../schema/schema.hx")
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (kind, rest) = if let Some(rest) = line.strip_prefix("N::") {
                (SchemaKind::Node, rest)
            } else if let Some(rest) = line.strip_prefix("V::") {
                (SchemaKind::Vector, rest)
            } else {
                (SchemaKind::Edge, line.strip_prefix("E::")?)
            };
            let name = rest.split_whitespace().next()?.trim_end_matches('{');
            Some((kind, name.to_string()))
        })
        .collect()
}

#[test]
fn inventory_covers_every_physical_declaration_exactly_once() {
    let schema = declared_in_hql();
    let inventory = declarations()
        .map(|declaration| (declaration.kind, declaration.name.to_string()))
        .collect::<BTreeSet<_>>();
    assert_eq!(schema, inventory);
    assert_eq!(NODES.len(), 22);
    assert_eq!(VECTORS.len(), 5);
    assert_eq!(EDGES.len(), 30);
}

#[test]
fn lifecycle_evidence_is_complete_and_points_to_real_e2e_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut count_keys = BTreeSet::new();
    for declaration in declarations() {
        assert!(
            !declaration.owner.trim().is_empty(),
            "missing owner for {}",
            declaration.physical_name()
        );
        assert!(
            !declaration.purpose.trim().is_empty(),
            "missing purpose for {}",
            declaration.physical_name()
        );
        assert!(
            count_keys.insert(declaration.count_key),
            "duplicate census key {}",
            declaration.count_key
        );
        match declaration.lifecycle {
            SchemaLifecycle::Active => {
                assert!(
                    declaration.producer.is_some(),
                    "missing producer for {}",
                    declaration.physical_name()
                );
                assert!(
                    declaration.consumer.is_some(),
                    "missing consumer for {}",
                    declaration.physical_name()
                );
                let e2e = declaration
                    .e2e
                    .expect("active declaration needs E2E evidence");
                assert!(
                    root.join(e2e).is_file(),
                    "E2E evidence does not exist for {}: {e2e}",
                    declaration.physical_name()
                );
            }
            SchemaLifecycle::Reserved => {
                assert!(
                    declaration.milestone.is_some(),
                    "missing milestone for {}",
                    declaration.physical_name()
                );
            }
            SchemaLifecycle::Deprecated => {
                assert!(
                    declaration.migration.is_some(),
                    "missing migration for {}",
                    declaration.physical_name()
                );
            }
        }
    }
}

#[test]
fn aggregate_census_queries_cover_every_inventory_key() {
    let queries = include_str!("../../schema/queries.hx");
    for query in [
        "getSchemaNodeCensus",
        "getSchemaVectorCensus",
        "getSchemaEdgeCensus",
    ] {
        assert!(
            queries.contains(&format!("QUERY {query}()")),
            "missing {query}"
        );
    }
    for declaration in declarations() {
        assert!(
            queries.contains(declaration.count_key),
            "missing census variable {}",
            declaration.count_key
        );
    }
}

#[test]
fn documentation_snapshot_matches_inventory_statuses() {
    let docs = include_str!("../../doc/data-model.md");
    for declaration in declarations() {
        let status = match declaration.lifecycle {
            SchemaLifecycle::Active => "active",
            SchemaLifecycle::Reserved => "reserved",
            SchemaLifecycle::Deprecated => "deprecated",
        };
        let row = format!(
            "| `{}` | `{}` | `{}` |",
            declaration.physical_name(),
            status,
            declaration.owner
        );
        assert!(
            docs.contains(&row),
            "data-model lifecycle snapshot is missing: {row}"
        );
    }
}

#[test]
fn nested_census_response_is_parsed_without_row_materialization() {
    let mut counts = BTreeMap::new();
    collect_named_counts(
        &serde_json::json!({"result": [{"memory_count": 12}, {"because_count": 4}], "ignored": [1, 2]}),
        &mut counts,
    );
    assert_eq!(counts.get("memory_count"), Some(&12));
    assert_eq!(counts.get("because_count"), Some(&4));
    assert_eq!(counts.len(), 2);
}
