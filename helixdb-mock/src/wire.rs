//! `HelixDB` v2.3.5 transport projection.
//!
//! The engine flattens node, edge, and vector properties next to transport
//! fields such as `id`, `label`, `from_node`, `data`, and `score`. The mock
//! retains nested properties internally so matching and mutation remain
//! coherent, then applies this projection only at the HTTP boundary.

use serde_json::Value;

/// Flatten every graph row inside a response while preserving wrapper fields.
pub(crate) fn flatten_rows(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(flatten_rows),
        Value::Object(object) => {
            for child in object.values_mut() {
                flatten_rows(child);
            }
            if object.contains_key("id")
                && object.contains_key("label")
                && let Some(Value::Object(properties)) = object.remove("properties")
            {
                for (name, property) in properties {
                    object.entry(name).or_insert(property);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn graph_properties_are_flattened_but_transport_fields_win() {
        let mut value = json!({"rows":[{
            "id":"transport-id","label":"Concept","properties":{
                "id":"property-id","concept_id":"Thing","level":1
            }
        }]});
        flatten_rows(&mut value);
        assert_eq!(
            value,
            json!({"rows":[{
                "id":"transport-id","label":"Concept","concept_id":"Thing","level":1
            }]})
        );
    }

    #[test]
    fn edge_and_vector_transport_fields_survive_projection() {
        let mut value = json!({
            "edge":{"id":"e","label":"REL","from_node":"a","to_node":"b",
                "properties":{"strength":80}},
            "vector":{"id":"v","label":"Embedding","data":[0.1],"score":0.9,
                "properties":{"created_at":"2026-01-01"}}
        });
        flatten_rows(&mut value);
        assert_eq!(value["edge"]["strength"], 80);
        assert_eq!(value["edge"]["from_node"], "a");
        assert_eq!(value["vector"]["data"], json!([0.1]));
        assert_eq!(value["vector"]["created_at"], "2026-01-01");
    }
}
