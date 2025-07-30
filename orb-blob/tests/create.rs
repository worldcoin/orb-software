use std::time::Duration;

use fixture::Fixture;
use reqwest::StatusCode;
use tokio::time;

mod fixture;

#[tokio::test]
async fn ok() {
    let fx = Fixture::new().await;

    let res = reqwest::get(format!("http://{}/health", fx.addr))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NO_CONTENT)
}
