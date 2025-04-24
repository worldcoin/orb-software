use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use orb_ota_backend::app::create_app;
use sqlx::PgPool;
use tower::ServiceExt; // for `oneshot`.

// A simple integration test that spins up the router in‑memory and makes a
// request with `tower::ServiceExt::oneshot`.
#[tokio::test]
async fn hello_world_endpoint_returns_200() {
    // A _lazy_ connection is enough because this test doesn't actually hit the
    // database – but we still want to satisfy the type signature.
    let pool =
        PgPool::connect_lazy(&std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://postgres:postgres@localhost/postgres".to_string()
        }))
        .expect("failed to create lazy pool");

    let app = create_app(pool);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("failed to get response");

    assert_eq!(response.status(), StatusCode::OK);

    let collected = response.into_body().collect().await.expect("body collect");
    let body = collected.to_bytes();
    assert_eq!(&body[..], b"Hello, World!");
}
