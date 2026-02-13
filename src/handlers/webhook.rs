use axum::http::header;
use axum::response::{IntoResponse, Response};

pub async fn sms_webhook() -> Response {
    (
        [(header::CONTENT_TYPE, "application/xml")],
        "<Response></Response>",
    )
        .into_response()
}
