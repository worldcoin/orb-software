use fixture::Fixture;
use reqwest::StatusCode;

mod fixture;

#[tokio::test]
async fn test_health_endpoint_returns_no_content() {
    let fx = Fixture::builder().build().await;

    let res = reqwest::get(format!("http://{}/health", fx.addr))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NO_CONTENT)
}
