use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::db::queries;
use crate::services::conversation;
use crate::services::inbox::record_inbox_event;
use crate::state::AppState;

static INBOX_HTML: &str = include_str!("../web/inbox.html");

pub async fn inbox_page() -> Html<&'static str> {
    Html(INBOX_HTML)
}

#[allow(clippy::result_large_err)]
fn check_auth(headers: &HeaderMap, expected_token: &str) -> Result<(), Response> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    if token != expected_token {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "unauthorized"})),
        )
            .into_response());
    }
    Ok(())
}

// GET /api/inbox/threads
pub async fn get_threads(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, Response> {
    check_auth(&headers, &state.config.admin_token)?;

    let threads = {
        let db = state.db.lock().unwrap();
        queries::get_inbox_threads(&db).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        })?
    };

    Ok(Json(serde_json::to_value(threads).unwrap_or_default()))
}

// GET /api/inbox/thread/:phone
#[derive(Deserialize)]
pub struct ThreadQuery {
    pub limit: Option<i64>,
}

pub async fn get_thread(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(phone): Path<String>,
    Query(query): Query<ThreadQuery>,
) -> Result<Json<serde_json::Value>, Response> {
    check_auth(&headers, &state.config.admin_token)?;

    let limit = query.limit.unwrap_or(200);
    let events = {
        let db = state.db.lock().unwrap();
        queries::get_thread_events(&db, &phone, limit).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        })?
    };

    Ok(Json(serde_json::to_value(events).unwrap_or_default()))
}

// POST /api/inbox/thread/:phone/read
pub async fn mark_read(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(phone): Path<String>,
) -> Result<Json<serde_json::Value>, Response> {
    check_auth(&headers, &state.config.admin_token)?;

    {
        let db = state.db.lock().unwrap();
        queries::mark_thread_read(&db, &phone).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        })?;
    }

    Ok(Json(serde_json::json!({"ok": true})))
}

// POST /api/inbox/reply
#[derive(Deserialize)]
pub struct ReplyRequest {
    pub phone: String,
    pub message: String,
}

pub async fn send_reply(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ReplyRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    check_auth(&headers, &state.config.admin_token)?;

    let phone = body.phone.trim().to_string();
    let message = body.message.trim().to_string();

    if phone.is_empty() || message.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "phone and message are required"})),
        )
            .into_response());
    }

    // Inject into conversation history
    if let Err(e) = conversation::inject_owner_reply(&state, &phone, &message) {
        tracing::error!(error = %e, "failed to inject owner reply");
    }

    // Send via messaging provider
    if let Err(e) = state.messaging.send_message(&phone, &message).await {
        tracing::error!(error = %e, "failed to send owner reply via messaging");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to send message: {e}")})),
        )
            .into_response());
    }

    // Record inbox event
    record_inbox_event(&state, &phone, "owner_reply", &message);

    Ok(Json(serde_json::json!({"ok": true})))
}

// GET /api/inbox/events — SSE stream
#[derive(Deserialize)]
pub struct SseQuery {
    pub token: Option<String>,
    pub last_id: Option<i64>,
}

pub async fn events_stream(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SseQuery>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, Response> {
    // Auth via query param (EventSource can't set headers)
    let token = query.token.as_deref().unwrap_or("");
    if token != state.config.admin_token {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "unauthorized"})),
        )
            .into_response());
    }

    let last_id = query.last_id.unwrap_or(0);

    // Catch up on missed events from DB
    let catchup_events = {
        let db = state.db.lock().unwrap();
        queries::get_inbox_events_since(&db, last_id).unwrap_or_default()
    };

    let rx = state.inbox_tx.subscribe();

    let catchup_stream = tokio_stream::iter(catchup_events.into_iter().map(|event| {
        let data = serde_json::to_string(&event).unwrap_or_default();
        Ok::<_, Infallible>(Event::default().data(data).event("inbox_event"))
    }));

    let live_stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(event) => {
            let data = serde_json::to_string(&event).unwrap_or_default();
            Some(Ok(Event::default().data(data).event("inbox_event")))
        }
        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(_)) => None,
    });

    let keepalive_stream = tokio_stream::StreamExt::map(
        tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(Duration::from_secs(30))),
        |_| Ok(Event::default().comment("keepalive")),
    );

    let combined = catchup_stream.chain(live_stream);
    let merged = StreamExt::merge(combined, keepalive_stream);

    Ok(Sse::new(merged))
}
