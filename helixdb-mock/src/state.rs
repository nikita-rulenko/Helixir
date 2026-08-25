//! Bounded state used to make write/read sequences coherent.

use crate::profile::Scenario;
use serde_json::{Map, Value};
use std::collections::VecDeque;

#[derive(Clone, Debug)]
struct StoredRecord {
    collection: String,
    value: Value,
}

/// FIFO record store with one global hard ceiling.
#[derive(Debug)]
pub(crate) struct StateStore {
    max_records: usize,
    records: VecDeque<StoredRecord>,
}

impl StateStore {
    /// Create a bounded store. Production configuration rejects zero; the
    /// defensive clamp keeps this internal primitive panic-free.
    #[must_use]
    pub(crate) fn new(max_records: usize) -> Self {
        Self {
            max_records: max_records.max(1),
            records: VecDeque::new(),
        }
    }

    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.records.len()
    }

    pub(crate) fn clear(&mut self) {
        self.records.clear();
    }

    /// Seed a bounded coherent graph skeleton. Aggregate census values live in
    /// the scenario profile; these rows exist only to satisfy real FIRST/id
    /// dependencies during daemon and RBAC flows.
    pub(crate) fn seed(&mut self, scenario: Scenario) {
        if matches!(scenario, Scenario::BootstrapEmpty | Scenario::Errors) {
            return;
        }
        self.insert("user", node("user", "user_id", "operator"));
        for group in ["default", "onboarding", "moirai"] {
            self.insert("rbac_group", node("rbac-group", "group_id", group));
        }
        self.insert(
            "rbac_config",
            serde_json::json!({
                "id":"config-1","label":"RbacConfig","properties":{
                    "config_id":"default","enabled":1,"migration_state":"active","migration_kind":"permanent",
                    "revision":1,"updated_by":"operator"
                }
            }),
        );
        self.insert(
            "rbac_assignment",
            serde_json::json!({
                "id":"assignment-operator","label":"RbacAssignment","properties":{
                    "assignment_id":"assignment-operator","subject_id":"operator",
                    "role":"admin","group_id":"","active":1
                }
            }),
        );
        self.insert(
            "rbac_dedup_group",
            node("dedup", "dedup_group_id", "default-dedup"),
        );
        self.insert("context", node("context", "context_id", "default-context"));
        if scenario == Scenario::Merge500 {
            for index in 0..500 {
                self.insert("memory", merge_memory(index));
            }
        } else {
            for index in 0..16 {
                self.insert(
                    "memory",
                    node("memory", "memory_id", &format!("mock-memory-{index}")),
                );
            }
        }
        for index in 0..8 {
            self.insert(
                "category",
                node("category", "category_id", &format!("mock-category-{index}")),
            );
            self.insert(
                "entity",
                node("entity", "entity_id", &format!("mock-entity-{index}")),
            );
            self.insert(
                "concept",
                node("concept", "concept_id", &format!("mock-concept-{index}")),
            );
        }
        self.insert("concept", node("concept", "concept_id", "Thing"));
        self.insert("agent", node("agent", "agent_id", "mock-agent-root"));
        if scenario == Scenario::IngestQueue {
            self.insert(
                "pending_input",
                node("pending", "pending_id", "mock-pending-0"),
            );
            self.insert(
                "memory_notice",
                node("notice", "notice_id", "mock-notice-0"),
            );
        }
    }

    /// Insert a record and evict the oldest record across every collection.
    pub(crate) fn insert(&mut self, collection: &str, value: Value) {
        self.records.push_back(StoredRecord {
            collection: normalize_collection(collection),
            value,
        });
        while self.records.len() > self.max_records {
            self.records.pop_front();
        }
    }

    /// Find the newest row that shares at least one identifier parameter.
    #[must_use]
    pub(crate) fn find_matching(&self, collection: &str, params: &Value) -> Option<Value> {
        let collection = normalize_collection(collection);
        self.records
            .iter()
            .rev()
            .filter(|record| record.collection == collection)
            .find(|record| identifiers_match(&record.value, params))
            .map(|record| record.value.clone())
    }

    /// Find a row across collections, used by UPDATE projections whose return
    /// variable does not carry the original node label.
    #[must_use]
    pub(crate) fn find_matching_any(&self, params: &Value) -> Option<Value> {
        self.records
            .iter()
            .rev()
            .find(|record| identifiers_match(&record.value, params))
            .map(|record| record.value.clone())
    }

    #[must_use]
    pub(crate) fn contains_lookup(&self, collection: &str, property: &str, value: &Value) -> bool {
        let collection = normalize_collection(collection);
        self.records.iter().any(|record| {
            if record.collection != collection {
                return false;
            }
            if property == "id" {
                return record.value.get("id") == Some(value);
            }
            record
                .value
                .get("properties")
                .and_then(|properties| properties.get(property))
                .or_else(|| record.value.get(property))
                == Some(value)
        })
    }

    /// Return the newest row matching one parsed HQL source lookup.
    #[must_use]
    pub(crate) fn find_lookup(
        &self,
        collection: &str,
        property: &str,
        value: &Value,
    ) -> Option<Value> {
        let collection = normalize_collection(collection);
        self.records
            .iter()
            .rev()
            .find(|record| {
                record.collection == collection
                    && if property == "id" {
                        record.value.get("id") == Some(value)
                    } else {
                        record
                            .value
                            .get("properties")
                            .and_then(|properties| properties.get(property))
                            .or_else(|| record.value.get(property))
                            == Some(value)
                    }
            })
            .map(|record| record.value.clone())
    }

    /// Return newest rows first, capped before cloning.
    #[must_use]
    pub(crate) fn list(&self, collection: &str, limit: usize) -> Vec<Value> {
        let collection = normalize_collection(collection);
        self.records
            .iter()
            .rev()
            .filter(|record| record.collection == collection)
            .take(limit)
            .map(|record| record.value.clone())
            .collect()
    }

    #[must_use]
    pub(crate) fn count(&self, collection: &str) -> usize {
        let collection = normalize_collection(collection);
        self.records
            .iter()
            .filter(|record| record.collection == collection)
            .count()
    }
}

fn identifiers_match(record: &Value, params: &Value) -> bool {
    let (Some(record), Some(params)) = (record.as_object(), params.as_object()) else {
        return false;
    };
    let properties = record
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or(record);
    let identifiers: Vec<_> = params
        .iter()
        .filter(|(key, value)| is_identifier(key) && is_scalar(value))
        .collect();
    !identifiers.is_empty()
        && identifiers
            .iter()
            .any(|(key, value)| properties.get(*key) == Some(*value))
}

fn node(label: &str, property: &str, value: &str) -> Value {
    let mut properties = Map::new();
    properties.insert(property.to_owned(), Value::String(value.to_owned()));
    properties.insert("name".to_owned(), Value::String(value.to_owned()));
    properties.insert("level".to_owned(), Value::from(1));
    properties.insert("description".to_owned(), Value::String(String::new()));
    properties.insert("parent_id".to_owned(), Value::String(String::new()));
    properties.insert("properties".to_owned(), Value::String("{}".to_owned()));
    properties.insert(
        "created_at".to_owned(),
        Value::String("2026-01-01T00:00:00Z".to_owned()),
    );
    properties.insert(
        "updated_at".to_owned(),
        Value::String("2026-01-01T00:00:00Z".to_owned()),
    );
    serde_json::json!({
        "id":format!("{label}-{value}"),
        "label":label,
        "properties":properties
    })
}

fn merge_memory(index: usize) -> Value {
    let memory_id = format!("merge-memory-{index:03}");
    serde_json::json!({
        "id": format!("merge-memory-node-{index:03}"),
        "label": "Memory",
        "properties": {
            "memory_id": memory_id,
            "content_key": format!("merge-content-key-{index:03}"),
            "rbac_scope": "rbac:group:default",
            "user_id": "mock-user",
            "content": format!("Deterministic merge candidate fact {index:03}."),
            "memory_type": "fact",
            "source": "helixdb-mock",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        }
    })
}

fn is_identifier(key: &str) -> bool {
    key == "id" || key.ends_with("_id") || key == "code" || key == "name"
}

fn is_scalar(value: &Value) -> bool {
    value.is_string() || value.is_number() || value.is_boolean()
}

/// Normalize return-field collection aliases without using them to infer JSON
/// cardinality. Cardinality comes exclusively from parsed HQL assignments.
#[must_use]
pub(crate) fn normalize_collection(value: &str) -> String {
    match value {
        "memories" | "memory" | "updated" => "memory",
        "users" | "user" | "knowers" => "user",
        "agents" | "agent" => "agent",
        "entities" | "entity" => "entity",
        "concepts" | "concept" | "thing" | "subtypes" => "concept",
        "categories" | "category" | "aliases_in" | "aliases_out" => "category",
        "contexts" | "context" => "context",
        "assignments" | "assignment" | "membership" | "rbac_assignment" => "rbac_assignment",
        "dedup_groups" | "dedup_group" | "rbac_dedup_group" => "rbac_dedup_group",
        "groups" | "group" | "rbac_group" => "rbac_group",
        other => other,
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn store_is_globally_bounded() {
        let mut store = StateStore::new(2);
        store.insert("memory", json!({"memory_id":"a"}));
        store.insert("user", json!({"user_id":"b"}));
        store.insert("memory", json!({"memory_id":"c"}));
        assert_eq!(store.len(), 2);
        assert!(
            store
                .find_matching("memory", &json!({"memory_id":"a"}))
                .is_none()
        );
        assert!(
            store
                .find_matching("memory", &json!({"memory_id":"c"}))
                .is_some()
        );
    }

    #[test]
    fn merge_fixture_has_five_hundred_unique_scoped_memories() {
        let mut store = StateStore::new(1024);
        store.seed(Scenario::Merge500);
        let memories = store.list("memory", 500);
        assert_eq!(memories.len(), 500);
        let ids = memories
            .iter()
            .filter_map(|memory| memory.get("id").and_then(Value::as_str))
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), 500);
        assert!(memories.iter().all(|memory| {
            let properties = &memory["properties"];
            properties["memory_id"].is_string()
                && properties["content_key"].is_string()
                && properties["source"] == "helixdb-mock"
                && properties["rbac_scope"] == "rbac:group:default"
        }));
    }
}
