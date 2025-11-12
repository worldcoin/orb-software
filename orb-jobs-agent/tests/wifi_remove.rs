use common::{fake_connd::MockConnd, fixture::JobAgentFixture};
use mockall::predicate::eq;
use orb_jobs_agent::shell::Host;
use orb_relay_messages::jobs::v1::JobExecutionStatus;

mod common;

#[tokio::test]
async fn it_removes_a_wifi_network() {
    // Arrange
    let fx = JobAgentFixture::new().await;

    let mut connd = MockConnd::new();
    connd
        .expect_remove_wifi_profile()
        .with(eq("ssid name with space".to_string()))
        .once()
        .return_const(Ok(()));

    fx.program().shell(Host).connd(connd).spawn().await;

    // Act
    fx.enqueue_job("wifi_remove ssid name with space")
        .await
        .wait_for_completion()
        .await;

    // Assert
    let result = fx.execution_updates.read().await;
    assert_eq!(
        result[0].status,
        JobExecutionStatus::Succeeded as i32,
        "{:?}",
        result[0]
    );
}
