use std::env;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub port: u16,
    pub database_url: String,
    pub admin_token: String,
    pub ollama_url: String,
    pub twilio_account_sid: String,
    pub twilio_auth_token: String,
    pub twilio_phone_number: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            port: env::var("PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3000),
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| "phonebook.db".to_string()),
            admin_token: env::var("ADMIN_TOKEN").unwrap_or_else(|_| "changeme".to_string()),
            ollama_url: env::var("OLLAMA_URL")
                .unwrap_or_else(|_| "http://localhost:11434".to_string()),
            twilio_account_sid: env::var("TWILIO_ACCOUNT_SID").unwrap_or_default(),
            twilio_auth_token: env::var("TWILIO_AUTH_TOKEN").unwrap_or_default(),
            twilio_phone_number: env::var("TWILIO_PHONE_NUMBER").unwrap_or_default(),
        }
    }
}
