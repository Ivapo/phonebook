mod config;
mod db;
mod errors;
mod handlers;
mod models;
mod services;
mod state;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use axum::routing::{get, post};
use axum::Router;
use tracing_subscriber::EnvFilter;

use config::AppConfig;
use services::ai::ollama::OllamaProvider;
use services::messaging::twilio::TwilioSmsProvider;
use state::AppState;

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
        .with_state(state);

    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("starting server on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
