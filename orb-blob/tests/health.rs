use fixture::Fixture;
use reqwest::StatusCode;

mod fixture;

#[tokio::test]
async fn ok() {
    let fx = Fixture::new().await;

    let res = reqwest::get(format!("http://{}/health", fx.addr))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NO_CONTENT)
}
