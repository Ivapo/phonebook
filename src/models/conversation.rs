use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub phone: String,
    pub messages: Vec<ConversationMessage>,
    pub state: String,
    pub last_activity: NaiveDateTime,
    pub expires_at: NaiveDateTime,
}
