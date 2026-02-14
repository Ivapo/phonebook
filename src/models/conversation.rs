use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConversationState {
    Idle,
    CollectingInfo,
    Confirming,
    Rescheduling,
    Cancelling,
}

impl ConversationState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConversationState::Idle => "idle",
            ConversationState::CollectingInfo => "collecting_info",
            ConversationState::Confirming => "confirming",
            ConversationState::Rescheduling => "rescheduling",
            ConversationState::Cancelling => "cancelling",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "collecting_info" => ConversationState::CollectingInfo,
            "confirming" => ConversationState::Confirming,
            "rescheduling" => ConversationState::Rescheduling,
            "cancelling" => ConversationState::Cancelling,
            _ => ConversationState::Idle,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingBooking {
    pub customer_name: Option<String>,
    pub date_time: Option<String>,
    pub duration_minutes: Option<i32>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationData {
    pub messages: Vec<ConversationMessage>,
    pub pending_booking: Option<PendingBooking>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub phone: String,
    pub messages: Vec<ConversationMessage>,
    pub state: ConversationState,
    pub pending_booking: Option<PendingBooking>,
    pub last_activity: NaiveDateTime,
    pub expires_at: NaiveDateTime,
}
