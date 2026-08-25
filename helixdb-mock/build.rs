mod build_lookup;
mod build_render;

use build_lookup::{is_direct_id_lookup, parse_required_lookup};
use build_render::render_registry;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::PathBuf;

const EXPECTED_QUERY_COUNT: usize = 192;

#[derive(Debug)]
struct Query {
    name: String,
    params: Vec<(String, String)>,
    returns: Vec<ReturnField>,
    mutation: bool,
    required_lookups: Vec<RequiredLookup>,
    source_line: usize,
}

#[derive(Debug)]
struct ReturnField {
    name: String,
    kind: ReturnKind,
    missing: MissingBehavior,
    literal: Option<String>,
    projection: Option<String>,
    row_kind: RowKind,
}

#[derive(Debug)]
struct RawQuery {
    name: String,
    params: Vec<(String, String)>,
    returns: Vec<String>,
    assignments: Vec<(String, String)>,
    statements: Vec<String>,
    source_line: usize,
}

#[derive(Clone, Copy, Debug)]
enum ReturnKind {
    Object,
    Array,
    Count,
    Literal,
}

#[derive(Clone, Copy, Debug)]
enum MissingBehavior {
    Empty,
    GraphError,
    Never,
}

#[derive(Clone, Copy, Debug)]
enum RowKind {
    Node,
    Edge,
    Vector,
    Scalar,
}

#[derive(Clone, Debug)]
struct RequiredLookup {
    collection: String,
    property: String,
    parameter: Option<String>,
    literal: Option<String>,
}

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let schema_path = manifest.join("../helixir/schema/queries.hx");
    println!("cargo:rerun-if-changed={}", schema_path.display());

    let source = fs::read_to_string(&schema_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", schema_path.display()));
    let queries = parse_queries(&source).unwrap_or_else(|error| panic!("invalid HQL: {error}"));
    validate_queries(&queries).unwrap_or_else(|error| panic!("invalid query registry: {error}"));

    let digest = format!("{:x}", Sha256::digest(source.as_bytes()));
    let generated = render_registry(&queries, &digest);
    let output = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("query_registry.rs");
    fs::write(&output, generated)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", output.display()));
}

fn parse_queries(source: &str) -> Result<Vec<Query>, String> {
    let mut queries = Vec::new();
    let mut current: Option<RawQuery> = None;

    for (index, raw_line) in source.lines().enumerate() {
        let line = raw_line.trim();
        if let Some(header) = line.strip_prefix("QUERY ") {
            if let Some(previous) = current.take() {
                queries.push(finish_query(previous)?);
            }
            let header = header
                .strip_suffix("=>")
                .ok_or_else(|| format!("line {}: query header must end with =>", index + 1))?
                .trim();
            let open = header
                .find('(')
                .ok_or_else(|| format!("line {}: missing parameter list", index + 1))?;
            let close = header
                .rfind(')')
                .ok_or_else(|| format!("line {}: missing closing parenthesis", index + 1))?;
            let name = header[..open].trim();
            if name.is_empty() {
                return Err(format!("line {}: empty query name", index + 1));
            }
            let params = parse_params(&header[open + 1..close], index + 1)?;
            current = Some(RawQuery {
                name: name.to_owned(),
                params,
                returns: Vec::new(),
                assignments: Vec::new(),
                statements: Vec::new(),
                source_line: index + 1,
            });
        } else if let Some(return_expr) = line.strip_prefix("RETURN ") {
            let Some(query) = current.as_mut() else {
                return Err(format!("line {}: RETURN outside QUERY", index + 1));
            };
            query
                .returns
                .extend(split_top_level(return_expr).into_iter().map(str::to_owned));
        } else if let Some((left, right)) = line.split_once("<-") {
            let Some(query) = current.as_mut() else {
                continue;
            };
            query
                .assignments
                .push((left.trim().to_owned(), right.trim().to_owned()));
        } else if let Some(query) = current.as_mut()
            && !line.is_empty()
            && !line.starts_with("//")
        {
            query.statements.push(line.to_owned());
        }
    }

    if let Some(previous) = current.take() {
        queries.push(finish_query(previous)?);
    }
    Ok(queries)
}

fn parse_params(input: &str, line: usize) -> Result<Vec<(String, String)>, String> {
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }
    split_top_level(input)
        .into_iter()
        .map(|item| {
            let (name, ty) = item
                .split_once(':')
                .ok_or_else(|| format!("line {line}: invalid parameter {item:?}"))?;
            Ok((name.trim().to_owned(), ty.trim().to_owned()))
        })
        .collect()
}

fn split_top_level(input: &str) -> Vec<&str> {
    let mut depth = 0_u32;
    let mut quoted = false;
    let mut start = 0;
    let mut values = Vec::new();
    for (index, ch) in input.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            '[' | '(' if !quoted => depth += 1,
            ']' | ')' if !quoted => depth = depth.saturating_sub(1),
            ',' if !quoted && depth == 0 => {
                values.push(input[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    values.push(input[start..].trim());
    values
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect()
}

fn finish_query(raw: RawQuery) -> Result<Query, String> {
    if raw.returns.is_empty() {
        return Err(format!(
            "line {}: {} has no RETURN",
            raw.source_line, raw.name
        ));
    }
    let assignment_map: HashMap<_, _> = raw.assignments.into_iter().collect();
    let returns = raw
        .returns
        .into_iter()
        .map(|expression| -> Result<_, String> {
            let field = if expression.starts_with('"') {
                "data".to_owned()
            } else {
                expression
                    .split("::{")
                    .next()
                    .unwrap_or(&expression)
                    .trim()
                    .to_owned()
            };
            let literal = expression
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .map(str::to_owned);
            let projection = expression
                .split_once("::{")
                .and_then(|(_, suffix)| suffix.strip_suffix('}'))
                .map(str::to_owned);
            let (kind, missing, mut row_kind) = if literal.is_some() {
                (ReturnKind::Literal, MissingBehavior::Never, RowKind::Scalar)
            } else {
                classify_variable(&raw.name, &field, &assignment_map, &mut HashSet::new())?
            };
            if projection.is_some() {
                row_kind = RowKind::Scalar;
            }
            Ok(ReturnField {
                name: field,
                kind,
                missing,
                literal,
                projection,
                row_kind,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mutation = is_mutation(&raw.name);
    let required_lookups = assignment_map
        .values()
        .chain(raw.statements.iter())
        .filter(|expression| expression.contains("::FIRST") || is_direct_id_lookup(expression))
        .map(|expression| parse_required_lookup(&raw.name, expression))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Query {
        name: raw.name,
        params: raw.params,
        returns,
        mutation,
        required_lookups,
        source_line: raw.source_line,
    })
}

fn classify_variable(
    query: &str,
    variable: &str,
    assignments: &HashMap<String, String>,
    visiting: &mut HashSet<String>,
) -> Result<(ReturnKind, MissingBehavior, RowKind), String> {
    if !visiting.insert(variable.to_owned()) {
        return Err(format!("{query}: cyclic assignment at {variable}"));
    }
    let expression = assignments
        .get(variable)
        .ok_or_else(|| format!("{query}: RETURN {variable} has no assignment"))?;
    let result = classify_expression(query, expression, assignments, visiting);
    visiting.remove(variable);
    result
}

fn classify_expression(
    query: &str,
    expression: &str,
    assignments: &HashMap<String, String>,
    visiting: &mut HashSet<String>,
) -> Result<(ReturnKind, MissingBehavior, RowKind), String> {
    if expression.ends_with("::COUNT") {
        return Ok((ReturnKind::Count, MissingBehavior::Never, RowKind::Scalar));
    }
    if let Some(base) = expression.split("::").next()
        && expression.contains("::UPDATE(")
    {
        let (_, missing, row_kind) = classify_variable(query, base, assignments, visiting)?;
        return Ok((ReturnKind::Object, missing, row_kind));
    }
    if expression.contains("::FIRST") || is_direct_id_lookup(expression) {
        return Ok((
            ReturnKind::Object,
            MissingBehavior::GraphError,
            root_row_kind(expression),
        ));
    }
    if let Some(base) = expression.split("::").next()
        && (expression.contains("::FromN") || expression.contains("::ToN"))
    {
        let (kind, missing, _) = classify_variable(query, base, assignments, visiting)?;
        return Ok((kind, missing, RowKind::Node));
    }
    if expression.starts_with("AddN<")
        || expression.starts_with("AddE<")
        || expression.starts_with("AddV<")
        || expression.contains("::UpsertN(")
        || expression.contains("::UpsertE(")
    {
        return Ok((
            ReturnKind::Object,
            MissingBehavior::Never,
            root_row_kind(expression),
        ));
    }
    if expression.starts_with("SearchV<")
        || expression.starts_with("SearchBM25<")
        || is_collection_root(expression)
        || expression.contains("::WHERE(")
        || expression.contains("::RANGE(")
        || expression.contains("::Out<")
        || expression.contains("::In<")
        || expression.contains("::OutE<")
        || expression.contains("::InE<")
    {
        let row_kind = if expression.contains("::OutE<") || expression.contains("::InE<") {
            RowKind::Edge
        } else if expression.contains("::Out<")
            || expression.contains("::In<")
            || expression.contains("::FromN")
            || expression.contains("::ToN")
        {
            RowKind::Node
        } else {
            root_row_kind(expression)
        };
        return Ok((ReturnKind::Array, MissingBehavior::Empty, row_kind));
    }
    if let Some(base) = expression.split("::").next()
        && assignments.contains_key(base)
    {
        return classify_variable(query, base, assignments, visiting);
    }
    Err(format!(
        "{query}: cannot derive response cardinality for {expression:?}; add an explicit parser rule"
    ))
}

fn root_row_kind(expression: &str) -> RowKind {
    if expression.starts_with("AddE<")
        || expression.starts_with("E<")
        || expression.contains("::OutE<")
        || expression.contains("::InE<")
    {
        RowKind::Edge
    } else if expression.starts_with("AddV<")
        || expression.starts_with("V<")
        || expression.starts_with("SearchV<")
    {
        RowKind::Vector
    } else {
        RowKind::Node
    }
}

fn is_collection_root(expression: &str) -> bool {
    let trimmed = expression.trim();
    (trimmed.starts_with("N<") || trimmed.starts_with("E<") || trimmed.starts_with("V<"))
        && !trimmed.contains('(')
}

fn is_mutation(name: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "add",
        "create",
        "ensure",
        "update",
        "set",
        "link",
        "grant",
        "revoke",
        "deactivate",
        "clear",
        "drop",
        "delete",
        "enqueue",
        "claim",
        "resolve",
        "tag",
        "heartbeat",
        "initialize",
        "mark",
        "unlink",
    ];
    PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

fn validate_queries(queries: &[Query]) -> Result<(), String> {
    if queries.len() != EXPECTED_QUERY_COUNT {
        return Err(format!(
            "expected {EXPECTED_QUERY_COUNT} queries, parsed {}",
            queries.len()
        ));
    }
    let mut names = HashSet::new();
    for query in queries {
        if !names.insert(&query.name) {
            return Err(format!("duplicate query {}", query.name));
        }
        let mut params = HashSet::new();
        for (name, _) in &query.params {
            if !params.insert(name) {
                return Err(format!("{} has duplicate parameter {name}", query.name));
            }
        }
        let mut returns = HashSet::new();
        for field in &query.returns {
            if !returns.insert(&field.name) {
                return Err(format!(
                    "{} has duplicate return field {}",
                    query.name, field.name
                ));
            }
        }
    }
    Ok(())
}
