//! Response encoding, redacted trace metadata, and request metrics.

use crate::metrics::{Observation, process_rss_bytes};
use crate::server::AppState;
use crate::trace::TraceEvent;
use axum::body::Body;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub(crate) struct Completion<'a> {
    pub request_id: u64,
    pub query: &'a str,
    pub status: StatusCode,
    pub body: Value,
    pub started: Instant,
    pub request_bytes: usize,
    pub request_hash: &'a str,
    pub parameter_names: &'a [String],
    pub state_records_before: usize,
    pub state_records_after: usize,
}

pub(crate) async fn finish(state: &AppState, completion: Completion<'_>) -> Response<Body> {
    let encoded = serde_json::to_vec(&completion.body).unwrap_or_else(|_| b"{}".to_vec());
    let latency_micros =
        u64::try_from(completion.started.elapsed().as_micros()).unwrap_or(u64::MAX);
    let response_hash = format!("{:x}", Sha256::digest(&encoded));
    let (response_shape, response_cardinality) = response_metadata(&completion.body);
    let output_cardinality = response_cardinality.values().sum();
    let state_delta = signed_delta(
        completion.state_records_before,
        completion.state_records_after,
    );
    let rss = process_rss_bytes();
    state
        .metrics
        .record(
            completion.query,
            Observation {
                request_bytes: completion.request_bytes,
                response_bytes: encoded.len(),
                output_cardinality,
                state_delta,
                latency_micros,
                error: !completion.status.is_success(),
            },
        )
        .await;
    let event = TraceEvent {
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis()),
        request_id: completion.request_id,
        query: completion.query,
        profile: state.profile.as_str(),
        status: completion.status.as_u16(),
        latency_micros,
        request_bytes: completion.request_bytes,
        response_bytes: encoded.len(),
        request_sha256: completion.request_hash,
        response_sha256: &response_hash,
        parameter_names: completion.parameter_names,
        response_shape: &response_shape,
        response_cardinality: &response_cardinality,
        state_records_before: completion.state_records_before,
        state_records_after: completion.state_records_after,
        process_rss_bytes: rss,
    };
    if let Err(error) = state.trace.record(&event).await {
        tracing::warn!(%error, "redacted trace write failed");
    }
    (
        completion.status,
        [("content-type", "application/json")],
        encoded,
    )
        .into_response()
}

fn response_metadata(body: &Value) -> (BTreeMap<String, String>, BTreeMap<String, usize>) {
    let mut shape = BTreeMap::new();
    let mut cardinality = BTreeMap::new();
    if let Some(fields) = body.as_object() {
        for (name, value) in fields {
            let (kind, count) = match value {
                Value::Array(values) => ("array", values.len()),
                Value::Object(_) => ("object", 1),
                Value::Null => ("null", 0),
                Value::Bool(_) => ("boolean", 1),
                Value::Number(_) => ("number", 1),
                Value::String(_) => ("string", 1),
            };
            shape.insert(name.clone(), kind.to_owned());
            cardinality.insert(name.clone(), count);
        }
    }
    (shape, cardinality)
}

fn signed_delta(before: usize, after: usize) -> i64 {
    let before = i64::try_from(before).unwrap_or(i64::MAX);
    let after = i64::try_from(after).unwrap_or(i64::MAX);
    after.saturating_sub(before)
}
