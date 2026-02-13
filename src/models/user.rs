use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub business_name: String,
    pub owner_name: String,
    pub owner_phone: String,
    pub twilio_account_sid: String,
    pub twilio_auth_token: String,
    pub twilio_phone_number: String,
    pub availability: Option<String>,
    pub timezone: String,
}
