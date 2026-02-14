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

fn test_state_with_sent() -> (Arc<AppState>, Arc<Mutex<Vec<(String, String)>>>) {
    let config = test_config();
    let conn = db::init_db(":memory:").unwrap();
    let sent = Arc::new(Mutex::new(vec![]));
    let messaging = MockMessaging {
        sent: Arc::clone(&sent),
    };
    let state = Arc::new(AppState {
        db: Arc::new(Mutex::new(conn)),
        config,
        llm: Box::new(MockLlm),
        messaging: Box::new(messaging),
        paused: AtomicBool::new(false),
    });
    (state, sent)
}

/// Build a POST to /webhook/sms from the owner phone number.
fn owner_sms_request(body: &str) -> Request<Body> {
    let encoded = body
        .replace('%', "%25")
        .replace('#', "%23")
        .replace('+', "%2B")
        .replace(' ', "+");
    Request::builder()
        .method("POST")
        .uri("/webhook/sms")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(format!(
            "From=%2B15559999999&To=%2B15551234567&Body={encoded}&MessageSid=SM_admin"
        )))
        .unwrap()
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

// ── Scheduling Validation Tests ──

#[tokio::test]
async fn test_booking_outside_business_hours_rejected() {
    let state = test_state();

    // Configure availability: Mon-Fri 09:00-17:00
    {
        let db = state.db.lock().unwrap();
        let user = phonebook::models::User {
            id: "default".to_string(),
            business_name: "Test Biz".to_string(),
            owner_name: "Alice".to_string(),
            owner_phone: "+15559999999".to_string(),
            twilio_account_sid: "".to_string(),
            twilio_auth_token: "".to_string(),
            twilio_phone_number: "+15551234567".to_string(),
            availability: Some(
                r#"{"slots":[{"day":"mon","start":"09:00","end":"17:00"},{"day":"tue","start":"09:00","end":"17:00"},{"day":"wed","start":"09:00","end":"17:00"},{"day":"thu","start":"09:00","end":"17:00"},{"day":"fri","start":"09:00","end":"17:00"}]}"#
                    .to_string(),
            ),
            timezone: "America/New_York".to_string(),
        };
        phonebook::db::queries::save_user(&db, &user).unwrap();
    }

    // The MockLlm returns a booking for Sunday (2025-06-15 is a Sunday) at 14:00
    // which is outside Mon-Fri availability
    let reply = phonebook::services::conversation::process_message(
        &state,
        "+15550001111",
        "I'd like to book an appointment",
    )
    .await
    .unwrap();

    // Should get a rejection about business hours
    assert!(
        reply.contains("outside our business hours") || reply.contains("available"),
        "Expected business hours rejection, got: {reply}"
    );
}

#[tokio::test]
async fn test_conflicting_booking_rejected() {
    let state = test_state();

    // Create an existing booking at the same time the MockLlm will propose (2025-06-15 14:00)
    {
        let db = state.db.lock().unwrap();
        let booking = phonebook::models::Booking {
            id: "conflict-1".to_string(),
            customer_phone: "+15559990000".to_string(),
            customer_name: Some("Existing".to_string()),
            date_time: chrono::NaiveDateTime::parse_from_str(
                "2025-06-15 14:00:00",
                "%Y-%m-%d %H:%M:%S",
            )
            .unwrap(),
            duration_minutes: 60,
            status: phonebook::models::BookingStatus::Confirmed,
            notes: None,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        };
        phonebook::db::queries::create_booking(&db, &booking).unwrap();
    }

    let reply = phonebook::services::conversation::process_message(
        &state,
        "+15550002222",
        "I'd like to book an appointment",
    )
    .await
    .unwrap();

    assert!(
        reply.contains("already booked") || reply.contains("different time"),
        "Expected conflict rejection, got: {reply}"
    );
}

#[tokio::test]
async fn test_valid_booking_succeeds() {
    let state = test_state();

    // No availability restrictions, no conflicting bookings
    // MockLlm proposes 2025-06-15 14:00
    let reply = phonebook::services::conversation::process_message(
        &state,
        "+15550003333",
        "I'd like to book an appointment",
    )
    .await
    .unwrap();

    // Should get the LLM's normal response (not a validation error)
    assert!(
        reply.contains("June 15") || reply.contains("2:00 PM") || reply.contains("book"),
        "Expected booking proposal, got: {reply}"
    );
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

// ── SMS Admin Command Tests ──

#[tokio::test]
async fn test_admin_sms_pause() {
    let (state, sent) = test_state_with_sent();
    let app = test_app(state.clone());

    let res = app.oneshot(owner_sms_request("#pause")).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    assert!(
        state.paused.load(std::sync::atomic::Ordering::SeqCst),
        "agent should be paused after #pause"
    );

    let messages = sent.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(
        messages[0].1.to_lowercase().contains("paused"),
        "reply should mention paused, got: {}",
        messages[0].1
    );
}

#[tokio::test]
async fn test_admin_sms_resume() {
    let (state, sent) = test_state_with_sent();
    let app = test_app(state.clone());

    let res = app.oneshot(owner_sms_request("#resume")).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    assert!(
        !state.paused.load(std::sync::atomic::Ordering::SeqCst),
        "agent should not be paused after #resume"
    );

    let messages = sent.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(
        messages[0].1.to_lowercase().contains("resumed"),
        "reply should mention resumed, got: {}",
        messages[0].1
    );
}

#[tokio::test]
async fn test_admin_sms_status() {
    let (state, sent) = test_state_with_sent();
    let app = test_app(state.clone());

    let res = app.oneshot(owner_sms_request("#status")).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let messages = sent.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(
        messages[0].1.contains("ACTIVE"),
        "reply should contain ACTIVE, got: {}",
        messages[0].1
    );
    assert!(
        messages[0].1.contains("Messages this hour"),
        "reply should contain message count, got: {}",
        messages[0].1
    );
    assert!(
        messages[0].1.contains("Blocked numbers"),
        "reply should contain blocked count, got: {}",
        messages[0].1
    );
}

#[tokio::test]
async fn test_admin_sms_block() {
    let (state, sent) = test_state_with_sent();
    let app = test_app(state.clone());

    let res = app
        .oneshot(owner_sms_request("#block +15551112222"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Verify number is blocked in DB
    {
        let db = state.db.lock().unwrap();
        assert!(
            phonebook::db::queries::is_blocked(&db, "+15551112222").unwrap(),
            "number should be blocked after #block"
        );
    }

    let messages = sent.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(
        messages[0].1.contains("Blocked"),
        "reply should contain Blocked, got: {}",
        messages[0].1
    );
}

#[tokio::test]
async fn test_admin_sms_unblock() {
    let (state, sent) = test_state_with_sent();

    // Block a number first
    {
        let db = state.db.lock().unwrap();
        phonebook::db::queries::block_number(&db, "+15551112222", Some("test"), false).unwrap();
    }

    let app = test_app(state.clone());
    let res = app
        .oneshot(owner_sms_request("#unblock +15551112222"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Verify number is unblocked in DB
    {
        let db = state.db.lock().unwrap();
        assert!(
            !phonebook::db::queries::is_blocked(&db, "+15551112222").unwrap(),
            "number should be unblocked after #unblock"
        );
    }

    let messages = sent.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(
        messages[0].1.contains("Unblocked"),
        "reply should contain Unblocked, got: {}",
        messages[0].1
    );
}

#[tokio::test]
async fn test_admin_sms_block_no_arg() {
    let (state, sent) = test_state_with_sent();
    let app = test_app(state.clone());

    let res = app.oneshot(owner_sms_request("#block")).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let messages = sent.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(
        messages[0].1.contains("Usage:"),
        "reply should contain Usage:, got: {}",
        messages[0].1
    );
}

#[tokio::test]
async fn test_admin_sms_unknown_command() {
    let (state, sent) = test_state_with_sent();
    let app = test_app(state.clone());

    let res = app.oneshot(owner_sms_request("#foo")).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let messages = sent.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(
        messages[0].1.contains("#pause") && messages[0].1.contains("#resume"),
        "reply should list available commands, got: {}",
        messages[0].1
    );
}

#[tokio::test]
async fn test_non_owner_hash_not_admin() {
    let (state, sent) = test_state_with_sent();
    let app = test_app(state.clone());

    // Send #pause from a non-owner number
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook/sms")
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(Body::from(
                    "From=%2B15550001111&To=%2B15551234567&Body=%23pause&MessageSid=SM_nonadmin",
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Agent should NOT be paused
    assert!(
        !state.paused.load(std::sync::atomic::Ordering::SeqCst),
        "agent should not be paused when non-owner sends #pause"
    );

    // Message should have gone to conversation engine (LLM reply sent back)
    let messages = sent.lock().unwrap();
    assert_eq!(messages.len(), 1, "conversation engine should have replied");
    assert_eq!(messages[0].0, "+15550001111", "reply should go to sender");
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
