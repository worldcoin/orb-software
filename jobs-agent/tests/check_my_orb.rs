use std::time::Duration;

use fake_orb::FakeOrb;
use fixture::JobAgentFixture;
use tokio::{fs, time};

mod fake_orb;
mod fixture;

#[tokio::test]
async fn test_docker() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let _ = fx.init_tracing();

    // Act
    fx.enqueue_job("check-my-orb").await;
    time::sleep(Duration::from_millis(10)).await; // give enough time exec cmd

    // Assert
    let actual = fx
        .execution_updates
        .read()
        .await
        .first()
        .unwrap()
        .std_out
        .clone();

    let expected =
        fs::read_to_string(FakeOrb::context_dir().join("check-my-orb_output.txt"))
            .await
            .unwrap();

    assert_eq!(actual, expected);
}
