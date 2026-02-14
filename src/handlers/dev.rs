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
}

pub async fn send_message(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<DevMessage>,
) -> Response {
    let from = payload.from_phone.trim().to_string();
    let body = payload.message.trim().to_string();

    // Owner admin commands
    if from == state.config.owner_phone && body.starts_with('#') {
        let reply = webhook::handle_admin_command(&state, &body).await;
        return Json(DevResponse {
            reply,
            success: true,
            error: None,
        })
        .into_response();
    }

    // Agent paused
    if state.paused.load(Ordering::SeqCst) {
        return Json(DevResponse {
            reply: "Agent is currently paused.".to_string(),
            success: true,
            error: None,
        })
        .into_response();
    }

    // Normal conversation
    match conversation::process_message(&state, &from, &body).await {
        Ok(reply) => Json(DevResponse {
            reply,
            success: true,
            error: None,
        })
        .into_response(),
        Err(e) => Json(DevResponse {
            reply: String::new(),
            success: false,
            error: Some(e.to_string()),
        })
        .into_response(),
    }
}
