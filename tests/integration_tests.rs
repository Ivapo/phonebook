use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use axum::Router;
use tower::ServiceExt;

use phonebook::config::AppConfig;
use phonebook::db;
use phonebook::handlers;
use phonebook::services::ai::{LlmProvider, Message};
use phonebook::services::messaging::MessagingProvider;
use phonebook::state::AppState;

// ── Mock Providers ──

struct MockLlm;

#[async_trait]
impl LlmProvider for MockLlm {
    async fn chat(&self, _system_prompt: &str, messages: &[Message]) -> anyhow::Result<String> {
        let last = messages.last().map(|m| m.content.as_str()).unwrap_or("");

        // Simple deterministic responses based on user message content
        if last.contains("book") || last.contains("appointment") {
            Ok(r#"{"intent":"book","customer_name":"Test User","requested_date":"2025-06-15","requested_time":"14:00","duration_minutes":60,"notes":null,"message_to_customer":"I'd like to book you for June 15 at 2:00 PM. Does that work?"}"#.to_string())
        } else if last.contains("yes") || last.contains("confirm") {
            Ok(r#"{"intent":"confirm","customer_name":null,"requested_date":null,"requested_time":null,"duration_minutes":null,"notes":null,"message_to_customer":"Great, you're all set for June 15 at 2:00 PM!"}"#.to_string())
        } else if last.contains("cancel") {
            Ok(r#"{"intent":"cancel","customer_name":null,"requested_date":null,"requested_time":null,"duration_minutes":null,"notes":null,"message_to_customer":"Your appointment has been cancelled."}"#.to_string())
        } else {
            Ok(r#"{"intent":"general_question","customer_name":null,"requested_date":null,"requested_time":null,"duration_minutes":null,"notes":null,"message_to_customer":"Hello! How can I help you today?"}"#.to_string())
        }
    }
}

struct MockMessaging {
    sent: Arc<Mutex<Vec<(String, String)>>>,
}

impl MockMessaging {
    fn new() -> Self {
        Self {
            sent: Arc::new(Mutex::new(vec![])),
        }
    }
}

#[async_trait]
impl MessagingProvider for MockMessaging {
    async fn send_message(&self, to: &str, body: &str) -> anyhow::Result<()> {
        self.sent
            .lock()
            .unwrap()
            .push((to.to_string(), body.to_string()));
        Ok(())
    }
}

// ── Helpers ──

fn test_config() -> AppConfig {
    AppConfig {
        port: 3000,
        database_url: ":memory:".to_string(),
        admin_token: "test-token".to_string(),
        ollama_url: "http://localhost:11434".to_string(),
        twilio_account_sid: "".to_string(),
        twilio_auth_token: "".to_string(), // empty = skip signature validation
        twilio_phone_number: "+15551234567".to_string(),
        owner_phone: "+15559999999".to_string(),
    }
}

fn test_state() -> Arc<AppState> {
    let config = test_config();
    let conn = db::init_db(":memory:").unwrap();
    Arc::new(AppState {
        db: Arc::new(Mutex::new(conn)),
        config,
        llm: Box::new(MockLlm),
        messaging: Box::new(MockMessaging::new()),
        paused: AtomicBool::new(false),
    })
}

fn test_app(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(handlers::health::health))
        .route("/webhook/sms", post(handlers::webhook::sms_webhook))
        .route("/admin", get(handlers::admin::admin_page))
        .route("/api/admin/status", get(handlers::admin::get_status))
        .route("/api/admin/bookings", get(handlers::admin::get_bookings))
        .route(
            "/api/admin/bookings/:id/cancel",
            post(handlers::admin::cancel_booking),
        )
        .route("/api/admin/blocked", get(handlers::admin::get_blocked))
        .route("/api/admin/block", post(handlers::admin::block_number))
        .route("/api/admin/unblock", post(handlers::admin::unblock_number))
        .route("/api/admin/pause", post(handlers::admin::pause_agent))
        .route("/api/admin/resume", post(handlers::admin::resume_agent))
        .route("/api/admin/settings", get(handlers::admin::get_settings))
        .route(
            "/api/admin/settings",
            post(handlers::admin::update_settings),
        )
        .route(
            "/calendar/:booking_id",
            get(handlers::calendar::download_ics),
        )
        .with_state(state)
}

// ── Admin API Tests ──

#[tokio::test]
async fn test_admin_requires_auth() {
    let state = test_state();
    let app = test_app(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/admin/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_admin_wrong_token() {
    let state = test_state();
    let app = test_app(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/admin/status")
                .header("Authorization", "Bearer wrong-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_admin_status() {
    let state = test_state();
    let app = test_app(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/admin/status")
                .header("Authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["paused"], false);
    assert_eq!(json["messages_this_hour"], 0);
    assert_eq!(json["blocked_count"], 0);
    assert_eq!(json["upcoming_bookings_count"], 0);
}

#[tokio::test]
async fn test_admin_pause_resume() {
    let state = test_state();

    // Pause
    let app = test_app(state.clone());
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/pause")
                .header("Authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Status should show paused
    let app = test_app(state.clone());
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/admin/status")
                .header("Authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["paused"], true);

    // Resume
    let app = test_app(state.clone());
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/resume")
                .header("Authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Verify resumed
    let app = test_app(state);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/admin/status")
                .header("Authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["paused"], false);
}

#[tokio::test]
async fn test_admin_block_unblock() {
    let state = test_state();

    // Block a number
    let app = test_app(state.clone());
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/block")
                .header("Authorization", "Bearer test-token")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    r#"{"phone":"+15551112222","reason":"spam"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Check blocked list
    let app = test_app(state.clone());
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/admin/blocked")
                .header("Authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(json.len(), 1);
    assert_eq!(json[0]["phone"], "+15551112222");
    assert_eq!(json[0]["reason"], "spam");

    // Unblock
    let app = test_app(state.clone());
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/unblock")
                .header("Authorization", "Bearer test-token")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"phone":"+15551112222"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Verify empty
    let app = test_app(state);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/admin/blocked")
                .header("Authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(json.len(), 0);
}

#[tokio::test]
async fn test_admin_settings() {
    let state = test_state();

    // Update settings
    let app = test_app(state.clone());
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/settings")
                .header("Authorization", "Bearer test-token")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    r#"{"business_name":"Test Biz","owner_name":"Alice","timezone":"America/New_York","availability":"Mon-Fri 9-5"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Read settings back
    let app = test_app(state);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/admin/settings")
                .header("Authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["business_name"], "Test Biz");
    assert_eq!(json["owner_name"], "Alice");
    assert_eq!(json["timezone"], "America/New_York");
    assert_eq!(json["availability"], "Mon-Fri 9-5");
}

// ── Webhook Tests ──

#[tokio::test]
async fn test_webhook_processes_message() {
    let state = test_state();
    let app = test_app(state);

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook/sms")
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(Body::from(
                    "From=%2B15551110000&To=%2B15551234567&Body=hello&MessageSid=SM123",
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("<Response>"));
}

#[tokio::test]
async fn test_webhook_blocked_number_ignored() {
    let state = test_state();

    // Block a number first
    {
        let db = state.db.lock().unwrap();
        phonebook::db::queries::block_number(&db, "+15551110000", Some("test"), false).unwrap();
    }

    let app = test_app(state);
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook/sms")
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(Body::from(
                    "From=%2B15551110000&To=%2B15551234567&Body=hello&MessageSid=SM123",
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_webhook_paused_agent_silent() {
    let state = test_state();
    state
        .paused
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let app = test_app(state);
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook/sms")
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(Body::from(
                    "From=%2B15551110000&To=%2B15551234567&Body=hello&MessageSid=SM123",
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

// ── Rate Limiting Tests ──

#[tokio::test]
async fn test_rate_limit_auto_blocks() {
    let state = test_state();

    // Send 16 messages from the same number (exceeds 15/hr limit)
    for i in 0..16 {
        let app = test_app(state.clone());
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/webhook/sms")
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "From=%2B15551110000&To=%2B15551234567&Body=msg{}&MessageSid=SM{}",
                        i, i
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    // Verify the number is now blocked
    let db = state.db.lock().unwrap();
    assert!(phonebook::db::queries::is_blocked(&db, "+15551110000").unwrap());
}

// ── Calendar .ics Tests ──

#[tokio::test]
async fn test_calendar_not_found() {
    let state = test_state();
    let app = test_app(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/calendar/nonexistent.ics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_calendar_download() {
    let state = test_state();

    // Create a booking
    {
        let db = state.db.lock().unwrap();
        let booking = phonebook::models::Booking {
            id: "test-booking-1".to_string(),
            customer_phone: "+15551110000".to_string(),
            customer_name: Some("Alice".to_string()),
            date_time: chrono::NaiveDateTime::parse_from_str(
                "2025-06-15 14:00:00",
                "%Y-%m-%d %H:%M:%S",
            )
            .unwrap(),
            duration_minutes: 60,
            status: phonebook::models::BookingStatus::Confirmed,
            notes: Some("Haircut".to_string()),
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        };
        phonebook::db::queries::create_booking(&db, &booking).unwrap();
    }

    let app = test_app(state);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/calendar/test-booking-1.ics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get("content-type").unwrap(),
        "text/calendar; charset=utf-8"
    );

    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("BEGIN:VCALENDAR"));
    assert!(text.contains("BEGIN:VEVENT"));
    assert!(text.contains("DTSTART:20250615T140000"));
    assert!(text.contains("DESCRIPTION:Haircut"));
}

// ── Booking CRUD via Admin API ──

#[tokio::test]
async fn test_admin_bookings_and_cancel() {
    let state = test_state();

    // Create a booking directly in DB
    {
        let db = state.db.lock().unwrap();
        let booking = phonebook::models::Booking {
            id: "bk-1".to_string(),
            customer_phone: "+15551110000".to_string(),
            customer_name: Some("Bob".to_string()),
            date_time: chrono::NaiveDateTime::parse_from_str(
                "2025-07-01 10:00:00",
                "%Y-%m-%d %H:%M:%S",
            )
            .unwrap(),
            duration_minutes: 30,
            status: phonebook::models::BookingStatus::Confirmed,
            notes: None,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        };
        phonebook::db::queries::create_booking(&db, &booking).unwrap();
    }

    // List bookings
    let app = test_app(state.clone());
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/admin/bookings")
                .header("Authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(json.len(), 1);
    assert_eq!(json[0]["customer_name"], "Bob");
    assert_eq!(json[0]["status"], "confirmed");

    // Cancel booking
    let app = test_app(state.clone());
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/bookings/bk-1/cancel")
                .header("Authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Verify cancelled
    let app = test_app(state);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/admin/bookings?status=cancelled")
                .header("Authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(json.len(), 1);
    assert_eq!(json[0]["status"], "cancelled");
}

// ── Health Check ──

#[tokio::test]
async fn test_health() {
    let state = test_state();
    let app = test_app(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

// ── Admin Page ──

#[tokio::test]
async fn test_admin_page_serves_html() {
    let state = test_state();
    let app = test_app(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("<!DOCTYPE html>"));
    assert!(text.contains("Booking Agent"));
}
