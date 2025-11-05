use common::{fake_connd::MockConnd, fixture::JobAgentFixture};
use orb_connd_dbus::NetConfig;
use orb_jobs_agent::shell::Host;
use orb_relay_messages::jobs::v1::JobExecutionStatus;
use serde_json::json;

mod common;

#[tokio::test]
async fn it_changes_netconfig() {
    // Arrange
    let fx = JobAgentFixture::new().await;

    let mut connd = MockConnd::new();
    connd
        .expect_netconfig_get()
        .once()
        .return_const(Ok(NetConfig {
            wifi: true,
            smart_switching: true,
            airplane_mode: false,
        }));

    fx.program().shell(Host).connd(connd).spawn().await;

    // Act
    fx.enqueue_job("netconfig_get")
        .await
        .wait_for_completion()
        .await;

    // Assert
    let result = fx.execution_updates.read().await;
    assert_eq!(
        result[0].status,
        JobExecutionStatus::Succeeded as i32,
        "{result:?}"
    );

    let expected = json!({
        "wifi": true,
        "smart_switching": true,
        "airplane_mode": false
    });

    let actual: serde_json::Value = serde_json::from_str(&result[0].std_out).unwrap();
    assert_eq!(actual, expected);
}
