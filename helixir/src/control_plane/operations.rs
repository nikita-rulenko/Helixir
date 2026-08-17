//! Authenticated web proxy for durable host-owned installation operations.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::Stream;
use serde::Deserialize;

use super::dto::ApiProblem;
use super::server::{AppState, require_host_operations, supervisor_error};
use super::supervisor::SupervisorClient;
use crate::installer::InstallOptions;
use crate::installer::operations::{OperationEvent, OperationSnapshot};

type Problem = (StatusCode, Json<ApiProblem>);

#[derive(Debug, Deserialize)]
pub(super) struct EventQuery {
    #[serde(default)]
    after: u64,
}

pub(super) async fn start(
    State(state): State<AppState>,
    Json(options): Json<InstallOptions>,
) -> Result<Json<OperationSnapshot>, Problem> {
    validate_operator(&state, &options)?;
    supervisor(&state)?
        .start_install(&options)
        .await
        .map(Json)
        .map_err(supervisor_error)
}

pub(super) async fn resume(
    State(state): State<AppState>,
    Path(operation_id): Path<String>,
    Json(options): Json<InstallOptions>,
) -> Result<Json<OperationSnapshot>, Problem> {
    validate_operator(&state, &options)?;
    supervisor(&state)?
        .resume_install(&operation_id, &options)
        .await
        .map(Json)
        .map_err(supervisor_error)
}

pub(super) async fn status(
    State(state): State<AppState>,
    Path(operation_id): Path<String>,
) -> Result<Json<OperationSnapshot>, Problem> {
    supervisor(&state)?
        .install_status(&operation_id)
        .await
        .map(Json)
        .map_err(supervisor_error)
}

pub(super) async fn events(
    State(state): State<AppState>,
    Path(operation_id): Path<String>,
    Query(query): Query<EventQuery>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, Problem> {
    let client = supervisor(&state)?.clone();
    client
        .install_status(&operation_id)
        .await
        .map_err(supervisor_error)?;
    let header_cursor = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.rsplit(':').next())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let stream = operation_stream(client, operation_id, query.after.max(header_cursor));
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("operation heartbeat"),
    ))
}

fn operation_stream(
    client: SupervisorClient,
    operation_id: String,
    cursor: u64,
) -> impl Stream<Item = Result<Event, Infallible>> {
    struct StreamState {
        client: SupervisorClient,
        operation_id: String,
        cursor: u64,
        pending: VecDeque<OperationEvent>,
        terminal: bool,
    }
    futures::stream::unfold(
        StreamState {
            client,
            operation_id,
            cursor,
            pending: VecDeque::new(),
            terminal: false,
        },
        |mut state| async move {
            loop {
                if let Some(event) = state.pending.pop_front() {
                    state.cursor = event.sequence;
                    let encoded = Event::default()
                        .id(event.event_id.clone())
                        .event("operation")
                        .json_data(&event)
                        .unwrap_or_else(|_| {
                            Event::default()
                                .event("error")
                                .data("event encoding failed")
                        });
                    return Some((Ok(encoded), state));
                }
                if state.terminal {
                    return None;
                }
                match state
                    .client
                    .install_events(&state.operation_id, state.cursor)
                    .await
                {
                    Ok(batch) => {
                        state.terminal = batch.terminal;
                        state.pending = batch.events.into();
                        if state.pending.is_empty() && !state.terminal {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, operation_id = %state.operation_id, "operation event proxy failed");
                        let event = Event::default()
                            .event("error")
                            .data("operation stream temporarily unavailable");
                        state.terminal = true;
                        return Some((Ok(event), state));
                    }
                }
            }
        },
    )
}

fn supervisor(state: &AppState) -> Result<&SupervisorClient, Problem> {
    require_host_operations(state)?.ok_or_else(|| {
        (
            StatusCode::NOT_IMPLEMENTED,
            Json(ApiProblem {
                code: "native_operation_journal_unavailable",
                message: "journaled web installation requires the authenticated host supervisor"
                    .to_string(),
            }),
        )
    })
}

fn validate_operator(state: &AppState, options: &InstallOptions) -> Result<(), Problem> {
    if options.rbac.operator_id == state.actor_id.as_ref() {
        return Ok(());
    }
    Err((
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(ApiProblem {
            code: "operator_mismatch",
            message: "the install operator must match the authenticated web actor".to_string(),
        }),
    ))
}
