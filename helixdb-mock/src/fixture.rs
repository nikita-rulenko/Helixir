//! Shape-faithful response construction for generated query specifications.

use crate::profile::{Profile, Scenario};
use crate::registry::{MissingBehavior, QuerySpec, ReturnKind, ReturnSpec, RowKind};
use crate::state::StateStore;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    #[error("GRAPH_ERROR: required source row is absent for {0}")]
    MissingRequired(String),
    #[error("response for {0} cannot fit configured byte ceiling")]
    ResponseTooLarge(String),
    #[error("response serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Build one complete response and update bounded state for mutations.
///
/// # Errors
///
/// Returns a graph error for absent required objects and a bounded-response
/// error if even a fully reduced response exceeds the configured ceiling.
pub(crate) fn build_response(
    query: &QuerySpec,
    params: &Value,
    state: &mut StateStore,
    profile: Profile,
    scenario: Scenario,
    seed: u64,
    max_bytes: usize,
) -> Result<Value, FixtureError> {
    let requested_limit = params
        .get("limit")
        .and_then(Value::as_i64)
        .and_then(|value| usize::try_from(value).ok())
        .or_else(|| {
            params
                .get("memory_ids")
                .and_then(Value::as_array)
                .map(Vec::len)
        });
    let params = bounded_value(params, 128);
    let rows = scenario.row_count(query.name, profile, requested_limit);
    let mut response = Map::new();

    for field in query.returns {
        let value = match field.kind {
            ReturnKind::Literal => Value::String(field.literal.unwrap_or_default().to_owned()),
            ReturnKind::Count => json!(count_value(
                query.name, field.name, state, profile, scenario
            )),
            ReturnKind::Object => object_value(
                query,
                field.name,
                field.row_kind,
                field.missing,
                &params,
                state,
                seed,
            )?,
            ReturnKind::Array => array_value(query, field, &params, state, scenario, rows, seed),
        };
        response.insert(field.name.to_owned(), value);
    }
    let mut response = Value::Object(response);
    crate::wire::flatten_rows(&mut response);
    enforce_response_bound(query.name, &mut response, max_bytes)?;
    Ok(response)
}

fn object_value(
    query: &QuerySpec,
    field: &str,
    row_kind: RowKind,
    missing: MissingBehavior,
    params: &Value,
    state: &mut StateStore,
    seed: u64,
) -> Result<Value, FixtureError> {
    let matched = if missing == MissingBehavior::GraphError {
        required_source(query, params, state)
            .or_else(|| state.find_matching(field, params))
            .or_else(|| state.find_matching_any(params))
    } else {
        None
    };
    if query.mutation {
        if missing == MissingBehavior::GraphError && matched.is_none() {
            return Err(FixtureError::MissingRequired(query.name.to_owned()));
        }
        let mut row = matched.unwrap_or_else(|| fixture_row(query.name, field, row_kind, 0, seed));
        merge_params(&mut row, params);
        state.insert(field, row.clone());
        Ok(row)
    } else if let Some(row) = matched {
        Ok(row)
    } else if missing == MissingBehavior::GraphError {
        Err(FixtureError::MissingRequired(query.name.to_owned()))
    } else {
        Ok(fixture_row(query.name, field, row_kind, 0, seed))
    }
}

fn required_source(query: &QuerySpec, params: &Value, state: &StateStore) -> Option<Value> {
    query.required_lookups.iter().find_map(|lookup| {
        let literal = lookup.literal.map(|value| Value::String(value.to_owned()));
        let value = lookup
            .parameter
            .and_then(|parameter| params.get(parameter))
            .or(literal.as_ref())?;
        state.find_lookup(lookup.collection, lookup.property, value)
    })
}

fn array_value(
    query: &QuerySpec,
    field: &ReturnSpec,
    params: &Value,
    state: &mut StateStore,
    scenario: Scenario,
    rows: usize,
    seed: u64,
) -> Value {
    let rows = if scenario == Scenario::Merge500
        && query.name == "getMemoryRbacScopesBatch"
        && field.name != "memories"
    {
        0
    } else {
        rows
    };
    let mut values = state.list(field.name, rows);
    if values.is_empty() {
        values = (0..rows)
            .map(|index| fixture_row(query.name, field.name, field.row_kind, index, seed))
            .collect();
    }
    if query.mutation {
        let mut row = wire_row_from_params(
            params,
            &synthetic_id(query.name, field.name, 0, seed),
            field.name,
            field.row_kind,
        );
        row = bounded_value(&row, 128);
        state.insert(field.name, row.clone());
        values = vec![row];
    }
    if let Some(property) = field.projection {
        Value::Array(
            values
                .into_iter()
                .map(|row| {
                    row.get("properties")
                        .and_then(|value| value.get(property))
                        .cloned()
                        .unwrap_or(Value::Null)
                })
                .collect(),
        )
    } else {
        Value::Array(values)
    }
}

fn merge_params(target: &mut Value, params: &Value) {
    let (Some(target), Some(params)) = (target.as_object_mut(), params.as_object()) else {
        return;
    };
    let target = target
        .entry("properties")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(target) = target.as_object_mut() else {
        return;
    };
    for (key, value) in params {
        target.insert(key.clone(), value.clone());
    }
}

fn count_value(
    query: &str,
    field: &str,
    state: &StateStore,
    profile: Profile,
    scenario: Scenario,
) -> usize {
    let collection = match query {
        "countAllMemories"
        | "countAgentMemories"
        | "countUserMemories"
        | "getMemoryUserCount"
        | "getContentKeyGroupUserCount" => "memory",
        "countAllUsers" => "user",
        "countAllEntities" => "entity",
        "countAllConcepts" => "concept",
        _ => field.strip_suffix("_count").unwrap_or(field),
    };
    let census_label = field.strip_suffix("_count").unwrap_or(collection);
    let census = scenario.census_count(profile, census_label);
    if census == 0 {
        state.count(collection)
    } else {
        census
    }
}

fn fixture_row(query: &str, field: &str, row_kind: RowKind, index: usize, seed: u64) -> Value {
    let id = synthetic_id(query, field, index, seed);
    let properties = json!({
        "memory_id": format!("mem_{id}"),
        "user_id": "mock-user",
        "principal_id": "mock-user",
        "agent_id": format!("mock-agent-{index}"),
        "group_id": "default",
        "category_id": format!("mock-category-{index}"),
        "entity_id": format!("mock-entity-{index}"),
        "concept_id": format!("mock-concept-{index}"),
        "name": format!("{field}-{index}"),
        "content": "deterministic helixdb-mock fixture",
        "memory_type": "fact",
        "source": "helixdb-mock",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "valid_from": "2026-01-01T00:00:00Z",
        "status": "done",
        "role": "worker",
        "active": 1,
        "certainty": 80,
        "importance": 50,
        "content_key": format!("mock-key-{index}"),
        "rbac_scope": "rbac:group:default",
        "metadata": "{}"
    });
    wire_row(&id, field, row_kind, properties)
}

fn wire_row_from_params(params: &Value, id: &str, label: &str, row_kind: RowKind) -> Value {
    wire_row(id, label, row_kind, params.clone())
}

fn wire_row(id: &str, label: &str, row_kind: RowKind, properties: Value) -> Value {
    match row_kind {
        RowKind::Node => json!({"id":id,"label":label,"properties":properties}),
        RowKind::Edge => json!({
            "id":id,"label":label,"from_node":"mock-from","to_node":"mock-to",
            "properties":properties
        }),
        RowKind::Vector => json!({
            "id":id,"label":label,"properties":properties,
            "data":[0.01,0.02,0.03,0.04],"score":0.91
        }),
        RowKind::Scalar => properties,
    }
}

fn synthetic_id(query: &str, field: &str, index: usize, seed: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(format!("{seed}:{query}:{field}:{index}").as_bytes());
    let mut id = String::with_capacity(16);
    for byte in &digest[..8] {
        id.push(char::from(HEX[usize::from(byte >> 4)]));
        id.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    id
}

fn bounded_value(value: &Value, string_limit: usize) -> Value {
    match value {
        Value::String(text) => Value::String(text.chars().take(string_limit).collect()),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .take(16)
                .map(|value| bounded_value(value, string_limit))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), bounded_value(value, string_limit)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn enforce_response_bound(
    query: &str,
    value: &mut Value,
    max_bytes: usize,
) -> Result<(), FixtureError> {
    loop {
        if serde_json::to_vec(value)?.len() <= max_bytes {
            return Ok(());
        }
        let Some(object) = value.as_object_mut() else {
            break;
        };
        let mut shrunk = false;
        for field in object.values_mut() {
            if let Some(array) = field.as_array_mut()
                && !array.is_empty()
            {
                array.truncate(array.len() / 2);
                shrunk = true;
            }
        }
        if !shrunk {
            break;
        }
    }
    Err(FixtureError::ResponseTooLarge(query.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{QUERY_SPECS, find_query};

    #[test]
    fn every_schema_endpoint_builds_a_bounded_shape_or_explicit_graph_error() {
        for query in QUERY_SPECS {
            let mut state = StateStore::new(128);
            let result = build_response(
                query,
                &json!({}),
                &mut state,
                Profile::Stress,
                Scenario::Baseline5k,
                7,
                262_144,
            );
            match result {
                Ok(value) => {
                    assert!(value.is_object(), "{} returned non-object", query.name);
                    assert!(serde_json::to_vec(&value).unwrap().len() <= 262_144);
                    for field in query.returns {
                        let value = &value[field.name];
                        match field.kind {
                            ReturnKind::Array => {
                                let rows = value.as_array().unwrap_or_else(|| {
                                    panic!("{}.{} must be array", query.name, field.name)
                                });
                                for row in rows {
                                    assert_wire_row(query.name, field.name, field.row_kind, row);
                                }
                            }
                            ReturnKind::Object => {
                                assert_wire_row(query.name, field.name, field.row_kind, value);
                            }
                            ReturnKind::Count => assert!(value.is_number()),
                            ReturnKind::Literal => assert!(value.is_string()),
                        }
                    }
                }
                Err(FixtureError::MissingRequired(name)) => assert_eq!(name, query.name),
                Err(error) => panic!("{}: {error}", query.name),
            }
        }
    }

    fn assert_wire_row(query: &str, field: &str, kind: RowKind, row: &Value) {
        let location = format!("{query}.{field}");
        if kind == RowKind::Scalar {
            return;
        }
        assert!(row.get("id").is_some(), "{location}: id");
        assert!(row.get("label").is_some(), "{location}: label");
        assert!(
            row.get("properties").is_none_or(Value::is_string),
            "{location}: flattened properties"
        );
        if kind == RowKind::Edge {
            assert!(row.get("from_node").is_some(), "{location}: from_node");
            assert!(row.get("to_node").is_some(), "{location}: to_node");
        }
        if kind == RowKind::Vector {
            assert!(row.get("data").is_some(), "{location}: data");
            assert!(row.get("score").is_some(), "{location}: score");
        }
    }

    #[test]
    fn literal_uses_real_data_wrapper() {
        let query = find_query("getHelixirSchemaVersion").unwrap();
        let value = build_response(
            query,
            &json!({}),
            &mut StateStore::new(1),
            Profile::Fast,
            Scenario::Baseline5k,
            1,
            4096,
        )
        .unwrap();
        assert_eq!(value, json!({"data":"helixir-rbac-moirai-v4"}));
    }

    #[test]
    fn first_lookup_without_state_is_graph_error() {
        let query = find_query("getMemory").unwrap();
        let error = build_response(
            query,
            &json!({"memory_id":"absent"}),
            &mut StateStore::new(1),
            Profile::Fast,
            Scenario::BootstrapEmpty,
            1,
            4096,
        )
        .unwrap_err();
        assert!(matches!(error, FixtureError::MissingRequired(_)));
    }

    #[test]
    fn recorded_baseline_count_is_not_inflated_by_seed_rows() {
        let query = find_query("countAllMemories").unwrap();
        let mut state = StateStore::new(128);
        state.seed(Scenario::Baseline5k);
        let value = build_response(
            query,
            &json!({}),
            &mut state,
            Profile::RecordedV235,
            Scenario::Baseline5k,
            1,
            4096,
        )
        .unwrap();
        assert_eq!(value, json!({"count": 5_883}));
    }

    #[test]
    fn literal_first_lookup_resolves_from_coherent_seed() {
        let query = find_query("getRbacConfig").unwrap();
        let mut state = StateStore::new(128);
        state.seed(Scenario::Baseline5k);
        let value = build_response(
            query,
            &json!({}),
            &mut state,
            Profile::Fast,
            Scenario::Baseline5k,
            1,
            4096,
        )
        .unwrap();
        assert_eq!(value["config"]["config_id"], "default");
    }
}
