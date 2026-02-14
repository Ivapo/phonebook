use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::config::AppConfig;
use crate::services::ai::LlmProvider;
use crate::services::messaging::MessagingProvider;

pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub config: AppConfig,
    pub llm: Box<dyn LlmProvider>,
    pub messaging: Box<dyn MessagingProvider>,
    pub paused: AtomicBool,
    pub dev_notifications: Mutex<Vec<String>>,
}
