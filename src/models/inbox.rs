use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InboxEvent {
    pub id: i64,
    pub phone: String,
    pub kind: String,
    pub content: String,
    pub is_read: bool,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct InboxThread {
    pub phone: String,
    pub last_message: String,
    pub last_kind: String,
    pub unread_count: i64,
    pub last_activity: String,
}
