use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use axum::routing::{get, post};
use axum::Router;
use tokio::sync::broadcast;
use tracing_subscriber::EnvFilter;

use phonebook::config::AppConfig;
use phonebook::db;
use phonebook::handlers;
use phonebook::services::ai::groq::GroqProvider;
use phonebook::services::ai::ollama::OllamaProvider;
use phonebook::services::ai::LlmProvider;
use phonebook::services::messaging::twilio::TwilioSmsProvider;
use phonebook::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = AppConfig::from_env();

    let conn = db::init_db(&config.database_url)?;

    let llm: Box<dyn LlmProvider> = match config.llm_provider.as_str() {
        "groq" => {
            anyhow::ensure!(!config.groq_api_key.is_empty(), "GROQ_API_KEY must be set when LLM_PROVIDER=groq");
            tracing::info!("using Groq LLM provider (model: {})", config.groq_model);
            Box::new(GroqProvider::new(config.groq_api_key.clone(), config.groq_model.clone()))
        }
        _ => {
            tracing::info!("using Ollama LLM provider (url: {})", config.ollama_url);
            Box::new(OllamaProvider::new(config.ollama_url.clone(), "llama3.2".to_string()))
        }
    };
    let messaging = TwilioSmsProvider::new(
        config.twilio_account_sid.clone(),
        config.twilio_auth_token.clone(),
        config.twilio_phone_number.clone(),
    );

    let (inbox_tx, _) = broadcast::channel(256);

    let state = Arc::new(AppState {
        db: Arc::new(Mutex::new(conn)),
        config: config.clone(),
        llm,
        messaging: Box::new(messaging),
        paused: AtomicBool::new(false),
        dev_notifications: Mutex::new(Vec::new()),
        inbox_tx,
    });

    let app = Router::new()
        .route("/health", get(handlers::health::health))
        .route("/webhook/sms", post(handlers::webhook::sms_webhook))
        .route("/app", get(handlers::admin::app_page))
        .route("/admin", get(handlers::admin::redirect_to_app))
        .route("/api/admin/status", get(handlers::admin::get_status))
        .route("/api/admin/activity", get(handlers::admin::get_activity))
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
        .route("/calendar/feed.ics", get(handlers::calendar::calendar_feed))
        .route(
            "/calendar/:booking_id",
            get(handlers::calendar::download_ics),
        )
        .route("/dev", get(handlers::dev::dev_page))
        .route("/api/dev/config", get(handlers::dev::dev_config))
        .route("/api/dev/message", post(handlers::dev::send_message))
        .route("/inbox", get(handlers::admin::redirect_to_app))
        .route("/api/inbox/threads", get(handlers::inbox::get_threads))
        .route(
            "/api/inbox/thread/:phone",
            get(handlers::inbox::get_thread),
        )
        .route(
            "/api/inbox/thread/:phone/read",
            post(handlers::inbox::mark_read),
        )
        .route("/api/inbox/reply", post(handlers::inbox::send_reply))
        .route("/api/inbox/events", get(handlers::inbox::events_stream))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("starting server on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
