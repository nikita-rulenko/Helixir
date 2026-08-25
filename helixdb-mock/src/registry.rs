//! Compile-time registry generated from Helixir's canonical HQL source.

use serde::Serialize;

/// One HQL parameter declared by a query.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ParamSpec {
    pub name: &'static str,
    pub hql_type: &'static str,
}

/// Coarse JSON shape of one HQL return field.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReturnKind {
    Object,
    Array,
    Count,
    Literal,
}

/// Behavior when a required HQL source row is absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingBehavior {
    /// Collections return an empty array.
    Empty,
    /// `FIRST` and direct-id lookups return a non-200 graph error.
    GraphError,
    /// Literals, counts and constructors cannot be absent.
    Never,
}

/// Wire row emitted by generated `HelixDB` v2 handlers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
    Node,
    Edge,
    Vector,
    Scalar,
}

/// One named HQL return field.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ReturnSpec {
    pub name: &'static str,
    pub kind: ReturnKind,
    pub missing: MissingBehavior,
    pub literal: Option<&'static str>,
    /// Field projected from every row (`RETURN rows::{memory_id}`).
    pub projection: Option<&'static str>,
    pub row_kind: RowKind,
}

/// One complete HTTP query endpoint.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct QuerySpec {
    pub name: &'static str,
    pub params: &'static [ParamSpec],
    pub returns: &'static [ReturnSpec],
    pub mutation: bool,
    /// Every source row that must exist before this query may execute.
    pub required_lookups: &'static [RequiredLookup],
    pub source_line: usize,
}

/// One required `FIRST` or direct-id dependency.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct RequiredLookup {
    pub collection: &'static str,
    pub property: &'static str,
    pub parameter: Option<&'static str>,
    pub literal: Option<&'static str>,
}

impl QuerySpec {
    /// Whether this endpoint invokes `HelixDB` vector search.
    #[must_use]
    pub fn is_vector(&self) -> bool {
        self.name.to_ascii_lowercase().contains("vector")
            || self.name.to_ascii_lowercase().contains("similar")
    }
}

include!(concat!(env!("OUT_DIR"), "/query_registry.rs"));

/// Find an endpoint by its exact HQL query name.
#[must_use]
pub fn find_query(name: &str) -> Option<&'static QuerySpec> {
    QUERY_SPECS.iter().find(|query| query.name == name)
}

/// Registry projection suitable for the loopback admin endpoint.
#[derive(Debug, Serialize)]
pub struct RegistryManifest {
    pub query_count: usize,
    pub expected_query_count: usize,
    pub schema_sha256: &'static str,
    pub queries: &'static [QuerySpec],
}

#[must_use]
pub fn manifest() -> RegistryManifest {
    RegistryManifest {
        query_count: QUERY_SPECS.len(),
        expected_query_count: EXPECTED_QUERY_COUNT,
        schema_sha256: SCHEMA_SHA256,
        queries: QUERY_SPECS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::collections::{BTreeMap, HashSet};
    use std::path::{Path, PathBuf};

    #[test]
    fn registry_is_complete_unique_and_current() {
        assert_eq!(QUERY_SPECS.len(), 192);
        assert_eq!(EXPECTED_QUERY_COUNT, 192);
        let names: HashSet<_> = QUERY_SPECS.iter().map(|query| query.name).collect();
        assert_eq!(names.len(), QUERY_SPECS.len());
        assert!(QUERY_SPECS.iter().all(|query| !query.returns.is_empty()));

        let source = include_bytes!("../../helixir/schema/queries.hx");
        assert_eq!(format!("{:x}", Sha256::digest(source)), SCHEMA_SHA256);
    }

    #[test]
    fn every_literal_rust_query_exists_in_hql() {
        let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../helixir/src");
        let mut files = Vec::new();
        collect_rust_files(&source_root, &mut files);
        let hql: HashSet<_> = QUERY_SPECS.iter().map(|query| query.name).collect();
        let mut unknown: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();

        for path in files {
            let source = std::fs::read_to_string(&path).unwrap();
            for query in literal_execute_queries(&source) {
                if query != "health" && !hql.contains(query.as_str()) {
                    unknown.entry(query).or_default().push(path.clone());
                }
            }
        }

        assert!(
            unknown.is_empty(),
            "Rust calls query routes absent from queries.hx: {unknown:#?}"
        );
    }

    fn collect_rust_files(directory: &Path, files: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect_rust_files(&path, files);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }

    fn literal_execute_queries(source: &str) -> Vec<String> {
        const NEEDLE: &str = ".execute_query";
        let mut queries = Vec::new();
        let mut cursor = 0;
        while let Some(relative) = source[cursor..].find(NEEDLE) {
            let start = cursor + relative + NEEDLE.len();
            let invocation = &source[start..];
            let Some(open) = invocation.find('(') else {
                break;
            };
            let arguments = invocation[open + 1..].trim_start();
            if let Some(literal) = arguments.strip_prefix('"')
                && let Some(close) = literal.find('"')
            {
                queries.push(literal[..close].to_owned());
            }
            cursor = start;
        }
        queries
    }

    #[test]
    fn schema_version_literal_is_preserved() {
        let query = find_query("getHelixirSchemaVersion").unwrap();
        assert_eq!(query.returns[0].literal, Some("helixir-rbac-moirai-v4"));
    }

    #[test]
    fn edge_mutations_retain_every_required_lookup() {
        let query = find_query("addMemoryRelation").unwrap();
        let parameters: HashSet<_> = query
            .required_lookups
            .iter()
            .filter_map(|lookup| lookup.parameter)
            .collect();
        assert_eq!(parameters, HashSet::from(["source_id", "target_id"]));
        assert!(
            query
                .required_lookups
                .iter()
                .all(|lookup| { lookup.collection == "memory" && lookup.property == "memory_id" })
        );
    }

    #[test]
    fn unassigned_drop_statement_retains_direct_id_lookup() {
        let query = find_query("deleteMemoryEmbedding").unwrap();
        assert!(query.required_lookups.iter().any(|lookup| {
            lookup.collection == "memory"
                && lookup.property == "id"
                && lookup.parameter == Some("memory_id")
        }));
    }

    #[test]
    fn literal_first_lookup_is_not_misclassified_as_a_parameter() {
        let query = find_query("getRbacConfig").unwrap();
        assert_eq!(query.required_lookups.len(), 1);
        assert_eq!(query.required_lookups[0].parameter, None);
        assert_eq!(query.required_lookups[0].literal, Some("default"));
    }

    #[test]
    fn generated_v235_cardinality_golden_set_is_exact() {
        for (query, field) in [
            ("getRecentMemories", "memories"),
            ("getRecentContexts", "contexts"),
            ("getAllCategories", "categories"),
            ("getCategoryGraphSnapshot", "categories"),
            ("getCategoryGraphSnapshot", "memories"),
        ] {
            let spec = find_query(query).unwrap();
            let value = spec
                .returns
                .iter()
                .find(|value| value.name == field)
                .unwrap();
            assert_eq!(value.kind, ReturnKind::Array, "{query}.{field}");
        }
        for (query, field) in [
            ("resolveMemoryContradictions", "updated"),
            ("clearRbacGroupDedupMembership", "updated"),
            ("revokeRbacRole", "updated"),
            ("revokeRbacGroupRole", "updated"),
            ("markNoticeDelivered", "updated"),
        ] {
            let spec = find_query(query).unwrap();
            let value = spec
                .returns
                .iter()
                .find(|value| value.name == field)
                .unwrap();
            assert_eq!(value.kind, ReturnKind::Object, "{query}.{field}");
        }
    }
}
