use axum::{routing::get, Router};
use sqlx::PgPool;
use std::sync::Arc;

async fn hello_world() -> &'static str {
    "Hello, World!"
}

/// Build an `axum::Router` with all routes for the service.
pub fn create_app(db: PgPool) -> Router {
    // For now we just store the pool inside an `Arc` and pass it as state so
    // handlers can grab a clone whenever they need DB access.
    let shared = Arc::new(db);

    Router::new()
        .route("/", get(hello_world))
        .with_state(shared)
}
