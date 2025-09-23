use axum::http::StatusCode;

pub async fn handler() -> StatusCode {
    StatusCode::NO_CONTENT
}
