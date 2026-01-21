use common::{fake_connd::MockConnd, fixture::JobAgentFixture};
use mockall::predicate::eq;
use orb_connd_dbus::NetConfig;
use orb_jobs_agent::shell::Host;
use orb_relay_messages::jobs::v1::JobExecutionStatus;
use serde_json::json;

mod common;

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test(flavor = "multi_thread")]
async fn it_changes_netconfig() {
    // Arrange
    let fx = JobAgentFixture::new().await;

    let mut connd = MockConnd::new();
    connd
        .expect_netconfig_set()
        .with(eq(true), eq(true), eq(false))
        .once()
        .return_const(Ok(NetConfig {
            wifi: true,
            smart_switching: true,
            airplane_mode: false,
        }));

    fx.program().shell(Host).connd(connd).spawn().await;

    let req = json!({
        "wifi": true,
        "smart_switching": true,
        "airplane_mode": false
    });

    // Act
    fx.enqueue_job(format!("netconfig_set {req}"))
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

    let expected = req;
    let actual: serde_json::Value = serde_json::from_str(&result[0].std_out).unwrap();
    assert_eq!(actual, expected);
}
