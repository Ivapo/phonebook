use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Book,
    Reschedule,
    Cancel,
    Confirm,
    Decline,
    GeneralQuestion,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedIntent {
    pub intent: Intent,
    pub customer_name: Option<String>,
    pub requested_date: Option<String>,
    pub requested_time: Option<String>,
    pub duration_minutes: Option<i32>,
    pub notes: Option<String>,
    pub message_to_customer: String,
}
