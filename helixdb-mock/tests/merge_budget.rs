use std::collections::HashSet;

use axum::Router;
use axum::body::{Body, to_bytes};
use helixdb_mock::config::Config;
use helixdb_mock::profile::{Profile, Scenario};
use helixdb_mock::server::{AppState, admin_router, data_router};
use http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt;

fn state() -> AppState {
    AppState::new(&Config {
        listen: "127.0.0.1:16969".parse().unwrap(),
        profile: Profile::Fast,
        scenario: Scenario::Merge500,
        seed: 17,
        max_response_bytes: 1_048_576,
        max_records: 4096,
        trace_path: None,
        admin_listen: None,
    })
    .unwrap()
}

async fn post(router: &Router, path: &str, body: Value) -> Value {
    let response = router
        .clone()
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    let body = to_bytes(response.into_body(), 2_097_152).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

fn calls(metrics: &Value, query: &str) -> u64 {
    metrics["by_query"][query]["calls"].as_u64().unwrap_or(0)
}

#[tokio::test]
async fn merge_500_candidate_boundary_stays_within_query_budget() {
    let state = state();
    let data = data_router(state.clone());
    let admin = admin_router(state);

    let recent = post(&data, "/getRecentMemories", json!({"limit": 500})).await;
    let memories = recent["memories"].as_array().unwrap();
    assert_eq!(memories.len(), 500);
    let memory_ids = memories
        .iter()
        .map(|memory| memory["memory_id"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(memory_ids.iter().collect::<HashSet<_>>().len(), 500);
    assert!(memories.iter().all(|memory| {
        memory["content_key"].is_string()
            && memory["source"] == "helixdb-mock"
            && memory["rbac_scope"] == "rbac:group:default"
    }));

    let scope_response = post(
        &data,
        "/getMemoryRbacScopesBatch",
        json!({"memory_ids": memory_ids}),
    )
    .await;
    let scoped_memories = scope_response["memories"].as_array().unwrap();
    assert_eq!(scoped_memories.len(), 500);
    assert_eq!(
        scoped_memories
            .iter()
            .filter_map(|memory| memory["id"].as_str())
            .collect::<HashSet<_>>()
            .len(),
        500
    );

    for _ in memories {
        let candidates = post(
            &data,
            "/smartVectorSearchWithChunks",
            json!({"query_vector": [0.1, 0.2, 0.3, 0.4], "limit": 25}),
        )
        .await;
        assert_eq!(candidates["memories"].as_array().unwrap().len(), 25);
    }

    let response = admin
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    let metrics: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(calls(&metrics, "getRecentMemories"), 1);
    assert_eq!(calls(&metrics, "getMemoryRbacScopesBatch"), 1);
    assert_eq!(calls(&metrics, "smartVectorSearchWithChunks"), 500);
    assert!(calls(&metrics, "smartVectorSearchWithChunks") <= 500);
    assert!(calls(&metrics, "restampContentKeyGroup") <= 499);

    for forbidden in [
        "searchMemoriesByBm25",
        "getConnectionsLevelBatch",
        "getSupersededBatch",
        "getMemoryStances",
        "getMemoryContradictionsFull",
        "getMemoryContradictions",
        "getContentKeyGroupUserCount",
        "getMemoriesByContentKey",
    ] {
        assert_eq!(calls(&metrics, forbidden), 0, "forbidden {forbidden}");
    }
}
