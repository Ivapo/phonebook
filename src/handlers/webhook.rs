use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use axum::Form;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha1::Sha1;

use crate::db::queries;
use crate::services::conversation;
use crate::state::AppState;

const PER_CUSTOMER_LIMIT: i64 = 15;
const GLOBAL_LIMIT: i64 = 100;

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct TwilioWebhookForm {
    #[serde(rename = "From")]
    pub from: String,
    #[serde(rename = "To")]
    pub to: String,
    #[serde(rename = "Body")]
    pub body: String,
    #[serde(rename = "MessageSid")]
    pub message_sid: Option<String>,
}

fn validate_twilio_signature(
    auth_token: &str,
    signature: &str,
    url: &str,
    params: &[(&str, &str)],
) -> bool {
    // Build the data to sign: URL + sorted params concatenated
    let mut data = url.to_string();
    let mut sorted_params = params.to_vec();
    sorted_params.sort_by(|a, b| a.0.cmp(b.0));
    for (key, value) in &sorted_params {
        data.push_str(key);
        data.push_str(value);
    }

    let mut mac = match Hmac::<Sha1>::new_from_slice(auth_token.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(data.as_bytes());
    let result = mac.finalize().into_bytes();
    let expected = base64::engine::general_purpose::STANDARD.encode(result);

    expected == signature
}

pub async fn sms_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(form): Form<TwilioWebhookForm>,
) -> Response {
    let from = form.from.trim().to_string();
    let body = form.body.trim().to_string();

    tracing::info!(from = %from, body = %body, "incoming SMS");

    // Validate Twilio signature (skip if auth token is empty — dev mode)
    if !state.config.twilio_auth_token.is_empty() {
        let signature = headers
            .get("x-twilio-signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if signature.is_empty() {
            tracing::warn!("missing X-Twilio-Signature header");
            return (
                axum::http::StatusCode::FORBIDDEN,
                "Missing signature",
            )
                .into_response();
        }

        // Reconstruct webhook URL — use X-Forwarded-Proto/Host if behind proxy
        let proto = headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("https");
        let host = headers
            .get("x-forwarded-host")
            .or_else(|| headers.get("host"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("localhost");
        let url = format!("{proto}://{host}/webhook/sms");

        let params = [
            ("From", from.as_str()),
            ("To", form.to.as_str()),
            ("Body", body.as_str()),
            ("MessageSid", form.message_sid.as_deref().unwrap_or("")),
        ];

        if !validate_twilio_signature(
            &state.config.twilio_auth_token,
            signature,
            &url,
            &params,
        ) {
            tracing::warn!("invalid Twilio signature");
            return (
                axum::http::StatusCode::FORBIDDEN,
                "Invalid signature",
            )
                .into_response();
        }
    }

    // 1. Check blocked
    {
        let db = state.db.lock().unwrap();
        match queries::is_blocked(&db, &from) {
            Ok(true) => {
                tracing::info!(from = %from, "blocked number, ignoring");
                return twiml_response();
            }
            Ok(false) => {}
            Err(e) => {
                tracing::error!(error = %e, "failed to check blocked status");
            }
        }
    }

    // 2. Increment rate limit counter
    let message_count = {
        let db = state.db.lock().unwrap();
        queries::increment_message_count(&db, &from).unwrap_or(1)
    };

    // 3. Per-customer rate limit check (>15/hr → auto-block)
    if message_count > PER_CUSTOMER_LIMIT {
        tracing::warn!(from = %from, count = message_count, "per-customer rate limit exceeded, auto-blocking");
        {
            let db = state.db.lock().unwrap();
            let _ = queries::block_number(&db, &from, Some("auto-blocked: rate limit exceeded"), true);
        }
        let alert = format!("Auto-blocked {from}: exceeded {PER_CUSTOMER_LIMIT} messages/hour ({message_count} msgs)");
        notify_owner(&state, &alert).await;
        return twiml_response();
    }

    // 4. Global rate limit check (>100/hr → pause agent)
    let global_count = {
        let db = state.db.lock().unwrap();
        queries::get_global_message_count(&db).unwrap_or(0)
    };
    if global_count > GLOBAL_LIMIT {
        tracing::warn!(global_count, "global rate limit exceeded, pausing agent");
        state.paused.store(true, Ordering::SeqCst);
        let alert = format!("Agent paused: global rate limit exceeded ({global_count} msgs/hour)");
        notify_owner(&state, &alert).await;
        return twiml_response();
    }

    // 5. Agent paused → silent ignore
    if state.paused.load(Ordering::SeqCst) {
        tracing::info!("agent is paused, ignoring message");
        return twiml_response();
    }

    // 6. Owner SMS with # prefix → admin command
    if from == state.config.owner_phone && body.starts_with('#') {
        let reply = handle_admin_command(&state, &body).await;
        if let Err(e) = state.messaging.send_message(&from, &reply).await {
            tracing::error!(error = %e, "failed to send admin reply");
        }
        return twiml_response();
    }

    // 7. Customer message → conversation engine
    match conversation::process_message(&state, &from, &body).await {
        Ok(reply) => {
            if let Err(e) = state.messaging.send_message(&from, &reply).await {
                tracing::error!(error = %e, "failed to send reply");
            }
        }
        Err(e) => {
            tracing::error!(error = %e, from = %from, "conversation processing failed");
            let fallback = "Sorry, I'm having trouble right now. Please try again in a moment.";
            let _ = state.messaging.send_message(&from, fallback).await;
        }
    }

    // 8. Cleanup old rate limit windows periodically
    {
        let db = state.db.lock().unwrap();
        let _ = queries::cleanup_old_windows(&db);
    }

    twiml_response()
}

pub async fn handle_admin_command(state: &Arc<AppState>, body: &str) -> String {
    let parts: Vec<&str> = body.splitn(2, ' ').collect();
    let command = parts[0].to_lowercase();
    let arg = parts.get(1).map(|s| s.trim());

    match command.as_str() {
        "#pause" => {
            state.paused.store(true, Ordering::SeqCst);
            "Agent paused. Send #resume to reactivate.".to_string()
        }
        "#resume" => {
            state.paused.store(false, Ordering::SeqCst);
            "Agent resumed and accepting messages.".to_string()
        }
        "#status" => {
            let paused = state.paused.load(Ordering::SeqCst);
            let db = state.db.lock().unwrap();
            let global_count = queries::get_global_message_count(&db).unwrap_or(0);
            let blocked = queries::list_blocked(&db).unwrap_or_default();
            format!(
                "Status: {}\nMessages this hour: {}\nBlocked numbers: {}",
                if paused { "PAUSED" } else { "ACTIVE" },
                global_count,
                blocked.len()
            )
        }
        "#block" => {
            if let Some(number) = arg {
                let db = state.db.lock().unwrap();
                match queries::block_number(&db, number, Some("blocked by owner"), false) {
                    Ok(_) => format!("Blocked {number}"),
                    Err(e) => format!("Error blocking: {e}"),
                }
            } else {
                "Usage: #block <phone_number>".to_string()
            }
        }
        "#unblock" => {
            if let Some(number) = arg {
                let db = state.db.lock().unwrap();
                match queries::unblock_number(&db, number) {
                    Ok(true) => format!("Unblocked {number}"),
                    Ok(false) => format!("{number} was not blocked"),
                    Err(e) => format!("Error unblocking: {e}"),
                }
            } else {
                "Usage: #unblock <phone_number>".to_string()
            }
        }
        _ => "Unknown command. Available: #pause, #resume, #status, #block <number>, #unblock <number>".to_string(),
    }
}

async fn notify_owner(state: &Arc<AppState>, message: &str) {
    // Always push to dev notification queue
    if let Ok(mut notifications) = state.dev_notifications.lock() {
        notifications.push(message.to_string());
    }

    if state.config.owner_phone.is_empty() {
        tracing::warn!("owner_phone not configured, skipping notification");
        return;
    }
    if let Err(e) = state
        .messaging
        .send_message(&state.config.owner_phone, message)
        .await
    {
        tracing::error!(error = %e, "failed to notify owner");
    }
}

fn twiml_response() -> Response {
    (
        [(header::CONTENT_TYPE, "application/xml")],
        "<Response></Response>",
    )
        .into_response()
}
