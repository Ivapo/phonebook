use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::config::AppConfig;
use crate::models::InboxEvent;
use crate::services::ai::LlmProvider;
use crate::services::messaging::MessagingProvider;

#[derive(Clone, Serialize)]
pub struct DevNotification {
    pub phone: Option<String>,
    pub kind: DevNotificationKind,
    pub content: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DevNotificationKind {
    CustomerMessage,
    AiReply,
    System,
}

pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub config: AppConfig,
    pub llm: Box<dyn LlmProvider>,
    pub messaging: Box<dyn MessagingProvider>,
    pub paused: AtomicBool,
    pub dev_notifications: Mutex<Vec<DevNotification>>,
    pub inbox_tx: broadcast::Sender<InboxEvent>,
}
