use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::db::queries;
use crate::models::BookingStatus;
use crate::state::AppState;

static ADMIN_HTML: &str = include_str!("../web/admin.html");

pub async fn admin_page() -> Html<&'static str> {
    Html(ADMIN_HTML)
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

// GET /api/admin/status
#[derive(Serialize)]
pub struct StatusResponse {
    paused: bool,
    messages_this_hour: i64,
    blocked_count: i64,
    upcoming_bookings_count: i64,
}

pub async fn get_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<StatusResponse>, Response> {
    check_auth(&headers, &state.config.admin_token)?;

    let paused = state.paused.load(Ordering::SeqCst);
    let stats = {
        let db = state.db.lock().unwrap();
        queries::get_dashboard_stats(&db).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        })?
    };

    Ok(Json(StatusResponse {
        paused,
        messages_this_hour: stats.messages_this_hour,
        blocked_count: stats.blocked_count,
        upcoming_bookings_count: stats.upcoming_bookings_count,
    }))
}

// GET /api/admin/bookings
#[derive(Deserialize)]
pub struct BookingsQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Serialize)]
pub struct BookingResponse {
    id: String,
    customer_phone: String,
    customer_name: Option<String>,
    date_time: String,
    duration_minutes: i32,
    status: String,
    notes: Option<String>,
    created_at: String,
    updated_at: String,
}

pub async fn get_bookings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<BookingsQuery>,
) -> Result<Json<Vec<BookingResponse>>, Response> {
    check_auth(&headers, &state.config.admin_token)?;

    let limit = query.limit.unwrap_or(50);
    let status_filter = query.status.as_deref();

    let bookings = {
        let db = state.db.lock().unwrap();
        queries::get_all_bookings(&db, status_filter, limit).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        })?
    };

    let response: Vec<BookingResponse> = bookings
        .into_iter()
        .map(|b| BookingResponse {
            id: b.id,
            customer_phone: b.customer_phone,
            customer_name: b.customer_name,
            date_time: b.date_time.format("%Y-%m-%d %H:%M:%S").to_string(),
            duration_minutes: b.duration_minutes,
            status: b.status.as_str().to_string(),
            notes: b.notes,
            created_at: b.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            updated_at: b.updated_at.format("%Y-%m-%d %H:%M:%S").to_string(),
        })
        .collect();

    Ok(Json(response))
}

// POST /api/admin/bookings/:id/cancel
pub async fn cancel_booking(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, Response> {
    check_auth(&headers, &state.config.admin_token)?;

    let updated = {
        let db = state.db.lock().unwrap();
        queries::update_booking_status(&db, &id, &BookingStatus::Cancelled).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        })?
    };

    if updated {
        Ok(Json(serde_json::json!({"ok": true})))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "booking not found"})),
        )
            .into_response())
    }
}

// GET /api/admin/blocked
#[derive(Serialize)]
pub struct BlockedResponse {
    phone: String,
    reason: Option<String>,
    is_auto: bool,
}

pub async fn get_blocked(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<BlockedResponse>>, Response> {
    check_auth(&headers, &state.config.admin_token)?;

    let blocked = {
        let db = state.db.lock().unwrap();
        queries::list_blocked(&db).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        })?
    };

    let response: Vec<BlockedResponse> = blocked
        .into_iter()
        .map(|(phone, reason, is_auto)| BlockedResponse {
            phone,
            reason,
            is_auto,
        })
        .collect();

    Ok(Json(response))
}

// POST /api/admin/block
#[derive(Deserialize)]
pub struct BlockRequest {
    pub phone: String,
    pub reason: Option<String>,
}

pub async fn block_number(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<BlockRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    check_auth(&headers, &state.config.admin_token)?;

    {
        let db = state.db.lock().unwrap();
        queries::block_number(&db, &body.phone, body.reason.as_deref(), false).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        })?;
    }

    Ok(Json(serde_json::json!({"ok": true})))
}

// POST /api/admin/unblock
#[derive(Deserialize)]
pub struct UnblockRequest {
    pub phone: String,
}

pub async fn unblock_number(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<UnblockRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    check_auth(&headers, &state.config.admin_token)?;

    let removed = {
        let db = state.db.lock().unwrap();
        queries::unblock_number(&db, &body.phone).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        })?
    };

    if removed {
        Ok(Json(serde_json::json!({"ok": true})))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "number not found in blocklist"})),
        )
            .into_response())
    }
}

// POST /api/admin/pause
pub async fn pause_agent(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, Response> {
    check_auth(&headers, &state.config.admin_token)?;
    state.paused.store(true, Ordering::SeqCst);
    Ok(Json(serde_json::json!({"ok": true, "paused": true})))
}

// POST /api/admin/resume
pub async fn resume_agent(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, Response> {
    check_auth(&headers, &state.config.admin_token)?;
    state.paused.store(false, Ordering::SeqCst);
    Ok(Json(serde_json::json!({"ok": true, "paused": false})))
}

// GET /api/admin/settings
#[derive(Serialize)]
pub struct SettingsResponse {
    business_name: String,
    owner_name: String,
    owner_phone: String,
    twilio_phone_number: String,
    twilio_configured: bool,
    availability: Option<String>,
    timezone: String,
}

pub async fn get_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<SettingsResponse>, Response> {
    check_auth(&headers, &state.config.admin_token)?;

    // Try to load user from DB, fall back to config
    let user = {
        let db = state.db.lock().unwrap();
        queries::get_user(&db, "default").ok().flatten()
    };

    match user {
        Some(u) => Ok(Json(SettingsResponse {
            business_name: u.business_name,
            owner_name: u.owner_name,
            owner_phone: u.owner_phone,
            twilio_phone_number: u.twilio_phone_number,
            twilio_configured: !u.twilio_account_sid.is_empty(),
            availability: u.availability,
            timezone: u.timezone,
        })),
        None => Ok(Json(SettingsResponse {
            business_name: String::new(),
            owner_name: String::new(),
            owner_phone: state.config.owner_phone.clone(),
            twilio_phone_number: state.config.twilio_phone_number.clone(),
            twilio_configured: !state.config.twilio_account_sid.is_empty(),
            availability: None,
            timezone: "UTC".to_string(),
        })),
    }
}

// POST /api/admin/settings
#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    pub business_name: Option<String>,
    pub owner_name: Option<String>,
    pub availability: Option<String>,
    pub timezone: Option<String>,
}

pub async fn update_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<UpdateSettingsRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    check_auth(&headers, &state.config.admin_token)?;

    let db = state.db.lock().unwrap();

    // Load existing user or create default
    let mut user = queries::get_user(&db, "default")
        .ok()
        .flatten()
        .unwrap_or(crate::models::User {
            id: "default".to_string(),
            business_name: String::new(),
            owner_name: String::new(),
            owner_phone: state.config.owner_phone.clone(),
            twilio_account_sid: state.config.twilio_account_sid.clone(),
            twilio_auth_token: state.config.twilio_auth_token.clone(),
            twilio_phone_number: state.config.twilio_phone_number.clone(),
            availability: None,
            timezone: "UTC".to_string(),
        });

    if let Some(name) = body.business_name {
        user.business_name = name;
    }
    if let Some(name) = body.owner_name {
        user.owner_name = name;
    }
    if let Some(avail) = body.availability {
        user.availability = Some(avail);
    }
    if let Some(tz) = body.timezone {
        user.timezone = tz;
    }

    queries::save_user(&db, &user).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response()
    })?;

    Ok(Json(serde_json::json!({"ok": true})))
}
