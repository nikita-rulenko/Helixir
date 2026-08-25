//! Deterministic, decision-LLM-free persistence for the golden read fixtures.

use super::{GOLDEN_USER, chains, corpus};
use helixir::core::HelixirClient;
use helixir::toolkit::mind_toolbox::reasoning::ReasoningType;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct FixtureRow {
    memory_id: String,
    content: String,
}

fn aged() -> Vec<(&'static str, &'static str, String, String)> {
    let now = chrono::Utc::now();
    let old = (now - chrono::Duration::days(365)).to_rfc3339();
    let recent_event = (now - chrono::Duration::days(7)).to_rfc3339();
    vec![
        (
            "gold_aged_created",
            "golden GOLDOLD: the legacy billing cron still runs quarterly reconciliation.",
            old.clone(),
            old.clone(),
        ),
        (
            "gold_aged_event",
            "golden GOLDEVENT: the quarterly reconciliation window moved to the first business day.",
            old,
            recent_event,
        ),
    ]
}

fn scoped_content_key(text: &str, memory_type: &str, scope: Option<&str>) -> String {
    let normalized = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut hasher = Sha256::new();
    if let Some(scope) = scope {
        hasher.update(scope.as_bytes());
        hasher.update([0]);
    }
    hasher.update(memory_type.to_lowercase().as_bytes());
    hasher.update([0]);
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

async fn fixture_rows(db: &helixir::HelixClient) -> Vec<FixtureRow> {
    let response: Value = db
        .execute_query(
            "getUserMemories",
            &json!({"user_id": GOLDEN_USER, "limit": 1_000}),
        )
        .await
        .expect("enumerate golden fixture memories");
    response
        .get("memories")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("getUserMemories returned no memories array: {response}"))
        .iter()
        .filter_map(|row| {
            Some(FixtureRow {
                memory_id: row.get("memory_id")?.as_str()?.to_string(),
                content: row.get("content")?.as_str()?.to_string(),
            })
        })
        .collect()
}

fn marker_ids(rows: &[FixtureRow], expected: &[(&str, &str)]) -> HashMap<String, Vec<String>> {
    expected
        .iter()
        .map(|(marker, text)| {
            let ids = rows
                .iter()
                .filter(|row| row.content == *text)
                .map(|row| row.memory_id.clone())
                .collect();
            ((*marker).to_string(), ids)
        })
        .collect()
}

fn exactly_one_id<'a>(ids: &'a HashMap<String, Vec<String>>, marker: &str) -> &'a str {
    match ids.get(marker).map(Vec::as_slice) {
        Some([memory_id]) => memory_id,
        Some(found) => panic!(
            "golden fixture invariant violated: marker {marker} has {} ids: {found:?}",
            found.len()
        ),
        None => panic!("golden fixture invariant violated: marker {marker} was not enumerated"),
    }
}

// Mirrors the typed HQL write contract explicitly so fixture setup cannot
// accidentally re-enter the product decision pipeline.
#[allow(clippy::too_many_arguments)]
async fn insert_fixture_memory(
    client: &HelixirClient,
    db: &helixir::HelixClient,
    actor: &str,
    scope: &helixir::core::RbacMemoryScope,
    memory_id: &str,
    content: &str,
    memory_type: &str,
    created_at: &str,
    valid_from: &str,
    embedding: Option<&[f32]>,
) {
    let fingerprint_scope = scope.fingerprint_scope();
    let response: Value = db
        .execute_query(
            "addMemoryKeyedScopedProtected",
            &json!({
                "memory_id": memory_id,
                "content_key": scoped_content_key(
                    content,
                    memory_type,
                    fingerprint_scope.as_deref(),
                ),
                "rbac_scope": fingerprint_scope.unwrap_or_default(),
                "user_id": GOLDEN_USER,
                "content": content,
                "memory_type": memory_type,
                "certainty": 90,
                "importance": 60,
                "created_at": created_at,
                "updated_at": created_at,
                "valid_from": valid_from,
                "context_tags": "golden-fixture",
                "source": "fixture",
                "metadata": "{}",
                "immutable": 0,
            }),
        )
        .await
        .unwrap_or_else(|error| panic!("insert fixture {memory_id}: {error}"));
    let internal_id = response
        .get("memory")
        .and_then(|memory| memory.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!("insert fixture {memory_id} returned no internal id: {response}")
        });

    if let Some(vector) = embedding {
        let _: Value = db
            .execute_query(
                "addMemoryEmbedding",
                &json!({
                    "memory_id": internal_id,
                    "vector_data": vector.iter().map(|value| f64::from(*value)).collect::<Vec<_>>(),
                    "embedding_model": client.embedder().model(),
                    "created_at": created_at,
                }),
            )
            .await
            .unwrap_or_else(|error| panic!("embed fixture {memory_id}: {error}"));
    }

    let _: Value = db
        .execute_query(
            "linkUserToMemoryWithStance",
            &json!({
                "user_id": GOLDEN_USER,
                "memory_id": memory_id,
                "context": "golden",
                "stance": "asserts",
                "certainty": 90,
                "linked_at": chrono::Utc::now().to_rfc3339(),
            }),
        )
        .await
        .unwrap_or_else(|error| panic!("link fixture owner {memory_id}: {error}"));
    client
        .rbac()
        .link_memory_to_scope(memory_id, scope, actor)
        .await
        .unwrap_or_else(|error| panic!("link fixture RBAC scope {memory_id}: {error}"));
}

async fn edge_target_count(
    db: &helixir::HelixClient,
    from_id: &str,
    to_id: &str,
    relation_type: ReasoningType,
) -> usize {
    let bucket = match relation_type {
        ReasoningType::Implies => "implies_out",
        ReasoningType::Because => "because_out",
        other => panic!(
            "golden fixture chain uses unsupported direct edge type {}",
            other.edge_name()
        ),
    };
    let response: Value = db
        .execute_query(
            "getMemoryLogicalConnections",
            &json!({"memory_id": from_id}),
        )
        .await
        .unwrap_or_else(|error| panic!("inspect golden edge {from_id}->{to_id}: {error}"));
    response
        .get(bucket)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("golden edge probe returned no {bucket} array: {response}"))
        .iter()
        .filter(|node| node.get("memory_id").and_then(Value::as_str) == Some(to_id))
        .count()
}

struct FixtureChain<'a> {
    from_marker: &'a str,
    to_marker: &'a str,
    from_id: &'a str,
    to_id: &'a str,
    relation_type: ReasoningType,
}

async fn ensure_chain(
    client: &HelixirClient,
    db: &helixir::HelixClient,
    actor: &str,
    chain: FixtureChain<'_>,
) {
    let FixtureChain {
        from_marker,
        to_marker,
        from_id,
        to_id,
        relation_type,
    } = chain;
    let before = edge_target_count(db, from_id, to_id, relation_type).await;
    assert!(
        before <= 1,
        "golden edge invariant violated before seed: {from_marker}->{to_marker} ({}) has {before} copies",
        relation_type.edge_name()
    );
    if before == 1 {
        return;
    }
    client
        .admin_as(actor)
        .await
        .expect("RBAC admin")
        .tooling()
        .add_typed_relation(from_id, to_id, relation_type, 80)
        .await
        .unwrap_or_else(|error| panic!("wire golden chain {from_marker}->{to_marker}: {error}"));
    for attempt in 0..30 {
        let count = edge_target_count(db, from_id, to_id, relation_type).await;
        assert!(
            count <= 1,
            "golden edge invariant violated after seed: {from_marker}->{to_marker} ({}) has {count} copies",
            relation_type.edge_name()
        );
        if count == 1 {
            return;
        }
        assert!(
            attempt < 29,
            "golden edge visibility timeout: {from_marker}->{to_marker} ({})",
            relation_type.edge_name()
        );
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

/// Idempotently seed the corpus + chains + aged fixtures, then wait for
/// search visibility (HelixDB snapshot lag: durable != immediately visible).
pub async fn ensure_seeded(client: &HelixirClient) -> usize {
    let actor = super::super::e2e_actor();
    let group = super::super::e2e_group();
    let authorized = client
        .rbac()
        .authorize_and_resolve_write_scope(&actor, GOLDEN_USER, Some(&group))
        .await
        .expect("authorize golden fixture scope");
    let admin = client.admin_as(&actor).await.expect("RBAC admin");
    let db = admin.db();
    let _: Value = db
        .execute_query(
            "ensureUser",
            &json!({"user_id": GOLDEN_USER, "name": GOLDEN_USER}),
        )
        .await
        .expect("ensure golden fixture owner");

    let normal = corpus();
    let aged = aged();
    let mut expected = normal
        .iter()
        .map(|(marker, text, _)| (*marker, *text))
        .collect::<Vec<_>>();
    expected.extend(
        aged.iter()
            .map(|(memory_id, text, _, _)| (*memory_id, *text)),
    );
    let before_ids = marker_ids(&fixture_rows(db).await, &expected);
    let missing_normal = normal
        .iter()
        .filter(|(marker, _, _)| before_ids.get(*marker).is_none_or(Vec::is_empty))
        .collect::<Vec<_>>();

    let embeddings = if missing_normal.is_empty() {
        Vec::new()
    } else {
        let missing_texts = missing_normal
            .iter()
            .map(|(_, text, _)| *text)
            .collect::<Vec<_>>();
        client
            .embedder()
            .generate_batch(&missing_texts, true)
            .await
            .expect("embed missing golden fixtures")
    };
    let now = chrono::Utc::now().to_rfc3339();
    for ((marker, text, memory_type), vector) in missing_normal.iter().zip(&embeddings) {
        insert_fixture_memory(
            client,
            db,
            &actor,
            &authorized.scope,
            &format!("gold_v1_{}", marker.to_ascii_lowercase()),
            text,
            memory_type,
            &now,
            &now,
            Some(vector),
        )
        .await;
    }

    let mut added_total = missing_normal.len();
    for (memory_id, text, created_at, valid_from) in &aged {
        if before_ids
            .get(*memory_id)
            .is_some_and(|ids| !ids.is_empty())
        {
            continue;
        }
        insert_fixture_memory(
            client,
            db,
            &actor,
            &authorized.scope,
            memory_id,
            text,
            "fact",
            created_at,
            valid_from,
            None,
        )
        .await;
        added_total += 1;
    }

    let mut ids = HashMap::new();
    for attempt in 0..30 {
        ids = marker_ids(&fixture_rows(db).await, &expected);
        let duplicates = ids
            .iter()
            .filter(|(_, memory_ids)| memory_ids.len() > 1)
            .map(|(marker, memory_ids)| format!("{marker}={memory_ids:?}"))
            .collect::<Vec<_>>();
        assert!(
            duplicates.is_empty(),
            "golden fixture invariant violated: duplicate canonical rows: {}",
            duplicates.join(", ")
        );
        if ids.values().all(|memory_ids| memory_ids.len() == 1) {
            break;
        }
        assert!(
            attempt < 29,
            "golden fixture visibility timeout; marker ids: {ids:?}"
        );
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    for (from_marker, to_marker, relation_type) in chains() {
        ensure_chain(
            client,
            db,
            &actor,
            FixtureChain {
                from_marker,
                to_marker,
                from_id: exactly_one_id(&ids, from_marker),
                to_id: exactly_one_id(&ids, to_marker),
                relation_type,
            },
        )
        .await;
    }

    readiness_probes(client, db, &actor).await;
    added_total
}

async fn readiness_probes(client: &HelixirClient, db: &helixir::HelixClient, actor: &str) {
    let mut bm25_probe = Value::Null;
    let mut hybrid_visible = false;
    let mut bm25_visible = false;
    for _ in 0..30 {
        bm25_probe = db
            .execute_query(
                "searchMemoriesByBm25",
                &json!({
                    "text": "legacy billing cron quarterly reconciliation",
                    "limit": 10,
                }),
            )
            .await
            .expect("direct GOLDOLD BM25 probe");
        bm25_visible = bm25_probe
            .get("memories")
            .and_then(Value::as_array)
            .is_some_and(|rows| {
                rows.iter().any(|row| {
                    row.get("content")
                        .and_then(Value::as_str)
                        .is_some_and(|content| content.contains("GOLDOLD"))
                })
            });
        let hits = client
            .search_as(
                actor,
                "payments sqlite postgres migration",
                GOLDEN_USER,
                helixir::core::helixir_client::SearchParams {
                    limit: Some(5),
                    search_mode: Some("full".to_string()),
                    scope: Some("personal".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap_or_default();
        hybrid_visible = hits.iter().any(|hit| hit.content.contains("GA1"));
        if bm25_visible && hybrid_visible {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    assert!(
        bm25_visible,
        "golden fixture/index failure: direct BM25 cannot find vectorless GOLDOLD: {bm25_probe}"
    );
    assert!(
        hybrid_visible,
        "golden fixture/index failure: full search cannot find embedded GA1 after readiness wait"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_lookup_reports_missing_and_duplicate_rows() {
        let expected = [("GA1", "one"), ("GA2", "two")];
        let rows = vec![
            FixtureRow {
                memory_id: "first".into(),
                content: "one".into(),
            },
            FixtureRow {
                memory_id: "second".into(),
                content: "one".into(),
            },
        ];
        let ids = marker_ids(&rows, &expected);
        assert_eq!(ids["GA1"], ["first", "second"]);
        assert!(ids["GA2"].is_empty());
    }
}
