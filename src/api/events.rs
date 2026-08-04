//! Event-query handlers and pagination limits.

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;

use crate::interfaces::EventStore;

use super::{app_error, ApiResponseError, AppState};

const MAX_EVENT_LIMIT: i32 = 10_000;
const DEFAULT_EVENT_LIMIT: i32 = 1_000;

#[derive(Deserialize, Default)]
pub(super) struct EventsQuery {
    after: Option<i64>,
    limit: Option<i32>,
}

pub(super) async fn events(
    State(state): State<AppState>,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<crate::interfaces::Event>>, ApiResponseError> {
    let limit = event_limit(query.limit);
    // Negative `after` is a backwards cursor: -1 starts at the durable tail,
    // otherwise it loads the page immediately before the absolute event ID.
    let events = if let Some(after_id) = query.after.filter(|id| *id < 0) {
        state
            .events
            .store()
            .query_all_before((-after_id).saturating_sub(1), limit)
            .await
    } else {
        state
            .events
            .query_all(query.after.unwrap_or_default(), limit)
            .await
    };
    events.map(Json).map_err(app_error)
}

pub(super) async fn session_events(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<crate::interfaces::Event>>, ApiResponseError> {
    let limit = event_limit(query.limit);
    let events = if let Some(after_id) = query.after.filter(|id| *id < 0) {
        state
            .events
            .store()
            .query_before(&session_id, (-after_id).saturating_sub(1), limit)
            .await
    } else {
        state
            .events
            .query(&session_id, query.after.unwrap_or_default(), limit)
            .await
    };
    events.map(Json).map_err(app_error)
}

fn event_limit(limit: Option<i32>) -> i32 {
    limit
        .filter(|limit| *limit > 0)
        .unwrap_or(DEFAULT_EVENT_LIMIT)
        .min(MAX_EVENT_LIMIT)
}
