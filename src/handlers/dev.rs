use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::extract::State;
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::services::conversation;
use crate::state::AppState;

use super::webhook;

pub async fn dev_page() -> Html<&'static str> {
    Html(include_str!("../web/dev_chat.html"))
}

#[derive(Deserialize)]
pub struct DevMessage {
    pub from_phone: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct DevResponse {
    pub reply: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub owner_notifications: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_delivery: Option<String>,
}

fn drain_notifications(state: &AppState) -> Vec<String> {
    state
        .dev_notifications
        .lock()
        .map(|mut n| n.drain(..).collect())
        .unwrap_or_default()
}

pub async fn send_message(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<DevMessage>,
) -> Response {
    let from = payload.from_phone.trim().to_string();
    let body = payload.message.trim().to_string();
    let is_owner = from == state.config.owner_phone;

    // Owner admin commands (# prefix)
    if is_owner && body.starts_with('#') {
        let reply = webhook::handle_admin_command(&state, &body).await;
        let notifications = drain_notifications(&state);
        return Json(DevResponse {
            reply,
            success: true,
            error: None,
            owner_notifications: notifications,
            customer_delivery: None,
        })
        .into_response();
    }

    // Owner non-# message → direct reply to customer (bypass AI)
    if is_owner {
        return Json(DevResponse {
            reply: String::new(),
            success: true,
            error: None,
            owner_notifications: Vec::new(),
            customer_delivery: Some(body),
        })
        .into_response();
    }

    // Agent paused
    if state.paused.load(Ordering::SeqCst) {
        let notifications = drain_notifications(&state);
        return Json(DevResponse {
            reply: "Agent is currently paused.".to_string(),
            success: true,
            error: None,
            owner_notifications: notifications,
            customer_delivery: None,
        })
        .into_response();
    }

    // Customer message → conversation engine
    match conversation::process_message(&state, &from, &body).await {
        Ok(reply) => {
            let notifications = drain_notifications(&state);
            Json(DevResponse {
                reply,
                success: true,
                error: None,
                owner_notifications: notifications,
                customer_delivery: None,
            })
            .into_response()
        }
        Err(e) => {
            let notifications = drain_notifications(&state);
            Json(DevResponse {
                reply: String::new(),
                success: false,
                error: Some(e.to_string()),
                owner_notifications: notifications,
                customer_delivery: None,
            })
            .into_response()
        }
    }
}
