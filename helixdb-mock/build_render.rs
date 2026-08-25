use super::{EXPECTED_QUERY_COUNT, Query};
use std::fmt::Write as _;

pub(super) fn render_registry(queries: &[Query], digest: &str) -> String {
    let mut out = String::new();
    writeln!(
        out,
        "pub const EXPECTED_QUERY_COUNT: usize = {EXPECTED_QUERY_COUNT};"
    )
    .unwrap();
    writeln!(out, "pub const SCHEMA_SHA256: &str = {digest:?};").unwrap();
    writeln!(out, "pub static QUERY_SPECS: &[QuerySpec] = &[").unwrap();
    for query in queries {
        writeln!(out, "    QuerySpec {{").unwrap();
        writeln!(out, "        name: {:?},", query.name).unwrap();
        writeln!(out, "        params: &[").unwrap();
        for (name, ty) in &query.params {
            writeln!(
                out,
                "            ParamSpec {{ name: {name:?}, hql_type: {ty:?} }},"
            )
            .unwrap();
        }
        writeln!(out, "        ],").unwrap();
        writeln!(out, "        returns: &[").unwrap();
        for field in &query.returns {
            writeln!(
                out,
                "            ReturnSpec {{ name: {:?}, kind: ReturnKind::{:?}, missing: MissingBehavior::{:?}, literal: {:?}, projection: {:?}, row_kind: RowKind::{:?} }},",
                field.name,
                field.kind,
                field.missing,
                field.literal,
                field.projection,
                field.row_kind
            )
            .unwrap();
        }
        writeln!(out, "        ],").unwrap();
        writeln!(out, "        mutation: {},", query.mutation).unwrap();
        writeln!(out, "        required_lookups: &[").unwrap();
        for lookup in &query.required_lookups {
            writeln!(
                out,
                "            RequiredLookup {{ collection: {:?}, property: {:?}, parameter: {:?}, literal: {:?} }},",
                lookup.collection, lookup.property, lookup.parameter, lookup.literal
            )
            .unwrap();
        }
        writeln!(out, "        ],").unwrap();
        writeln!(out, "        source_line: {},", query.source_line).unwrap();
        writeln!(out, "    }},").unwrap();
    }
    writeln!(out, "];").unwrap();
    out
}
