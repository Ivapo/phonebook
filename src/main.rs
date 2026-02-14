use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use axum::routing::{get, post};
use axum::Router;
use tracing_subscriber::EnvFilter;

use phonebook::config::AppConfig;
use phonebook::db;
use phonebook::handlers;
use phonebook::services::ai::ollama::OllamaProvider;
use phonebook::services::messaging::twilio::TwilioSmsProvider;
use phonebook::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = AppConfig::from_env();

    let conn = db::init_db(&config.database_url)?;

    let llm = OllamaProvider::new(config.ollama_url.clone(), "llama3.2".to_string());
    let messaging = TwilioSmsProvider::new(
        config.twilio_account_sid.clone(),
        config.twilio_auth_token.clone(),
        config.twilio_phone_number.clone(),
    );

    let state = Arc::new(AppState {
        db: Arc::new(Mutex::new(conn)),
        config: config.clone(),
        llm: Box::new(llm),
        messaging: Box::new(messaging),
        paused: AtomicBool::new(false),
    });

    let app = Router::new()
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
        .route("/dev", get(handlers::dev::dev_page))
        .route("/api/dev/message", post(handlers::dev::send_message))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("starting server on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
