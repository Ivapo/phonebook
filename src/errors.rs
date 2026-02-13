use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("AI provider error: {0}")]
    Ai(String),

    #[error("messaging error: {0}")]
    Messaging(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("rate limited: {0}")]
    RateLimited(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Config(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Ai(_) => StatusCode::BAD_GATEWAY,
            AppError::Messaging(_) => StatusCode::BAD_GATEWAY,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
        };

        let body = serde_json::json!({ "error": self.to_string() });
        (status, axum::Json(body)).into_response()
    }
}
