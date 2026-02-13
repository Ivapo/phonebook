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
