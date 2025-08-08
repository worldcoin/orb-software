use common::{fake_orb::FakeOrb, fixture::JobAgentFixture};
use std::time::Duration;
use tokio::{fs, time};

mod common;

#[tokio::test]
async fn it_executes_check_my_orb() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    fx.spawn_program(FakeOrb::new().await);

    // Act
    fx.enqueue_job("check_my_orb").await;
    time::sleep(Duration::from_millis(500)).await; // give enough time exec cmd
                                                   // TODO: USE NOTIFY FOR FLAKYNESS

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
