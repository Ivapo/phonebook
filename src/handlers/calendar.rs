use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::db::queries;
use crate::services::calendar::generate_ics;
use crate::state::AppState;

pub async fn download_ics(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
) -> Response {
    // Strip .ics suffix if present
    let booking_id = raw_id.strip_suffix(".ics").unwrap_or(&raw_id);

    let booking = {
        let db = state.db.lock().unwrap();
        match queries::get_booking_by_id(&db, booking_id) {
            Ok(Some(b)) => b,
            Ok(None) => {
                return (StatusCode::NOT_FOUND, "Booking not found").into_response();
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to load booking for .ics");
                return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
            }
        }
    };

    // Get business name from user settings or fall back
    let business_name = {
        let db = state.db.lock().unwrap();
        queries::get_user(&db, "default")
            .ok()
            .flatten()
            .map(|u| u.business_name)
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Booking".to_string())
    };

    let ics = generate_ics(&booking, &business_name);
    let filename = format!("booking-{}.ics", booking_id);

    (
        [
            (header::CONTENT_TYPE, "text/calendar; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{filename}\""),
            ),
        ],
        ics,
    )
        .into_response()
}
