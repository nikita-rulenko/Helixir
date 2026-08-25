use axum::body::Body;
use axum::body::to_bytes;
use helixdb_mock::config::Config;
use helixdb_mock::server::{AppState, data_router};
use http::{Request, StatusCode};
use tower::ServiceExt;

fn state() -> AppState {
    AppState::new(&Config {
        listen: "127.0.0.1:16969".parse().unwrap(),
        profile: helixdb_mock::profile::Profile::Fast,
        scenario: helixdb_mock::profile::Scenario::Baseline5k,
        seed: 17,
        max_response_bytes: 262_144,
        max_records: 4096,
        trace_path: None,
        admin_listen: None,
    })
    .unwrap()
}

#[tokio::test]
async fn edge_write_requires_both_endpoint_nodes() {
    let state = state();
    let request = Request::post("/addMemoryRelation")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{
                "source_id":"mock-memory-0",
                "target_id":"absent",
                "relation_type":"SUPPORTS",
                "strength":80,
                "created_at":"2026-01-01T00:00:00Z",
                "metadata":"{}"
            }"#,
        ))
        .unwrap();
    let response = data_router(state.clone()).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let request = Request::post("/addMemoryRelation")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{
                "source_id":"mock-memory-0",
                "target_id":"mock-memory-1",
                "relation_type":"SUPPORTS",
                "strength":80,
                "created_at":"2026-01-01T00:00:00Z",
                "metadata":"{}"
            }"#,
        ))
        .unwrap();
    let response = data_router(state).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(body["relation"]["from_node"].is_string());
    assert!(body["relation"]["to_node"].is_string());
    assert!(body["relation"].get("properties").is_none());
}

#[tokio::test]
async fn node_rows_match_flat_v235_transport() {
    let response = data_router(state())
        .oneshot(
            Request::post("/getAllConcepts")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 16_384).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let concept = &body["concepts"][0];
    for field in [
        "id",
        "label",
        "concept_id",
        "name",
        "level",
        "description",
        "parent_id",
    ] {
        assert!(concept.get(field).is_some(), "missing flat field {field}");
    }
    assert!(concept["properties"].is_string());
}
