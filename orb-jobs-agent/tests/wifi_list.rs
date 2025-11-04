use common::{fake_connd::MockConnd, fixture::JobAgentFixture};
use orb_connd_dbus::WifiProfile;
use orb_jobs_agent::shell::Host;
use orb_relay_messages::jobs::v1::JobExecutionStatus;

mod common;

#[tokio::test]
async fn it_lists_wifi_profiles() {
    // Arrange
    let fx = JobAgentFixture::new().await;

    let expected = vec![
        WifiProfile {
            ssid: "apple".into(),
            sec: "wpa2".into(),
            psk: "87654321".into(),
        },
        WifiProfile {
            ssid: "pineapple".into(),
            sec: "wpa3".into(),
            psk: "12345678".into(),
        },
    ];

    let mut connd = MockConnd::new();
    connd
        .expect_list_wifi_profiles()
        .once()
        .return_const(Ok(expected.clone()));

    fx.program().shell(Host).connd(connd).spawn().await;

    // Act
    fx.enqueue_job("wifi_list")
        .await
        .wait_for_completion()
        .await;

    // Assert
    let result = fx.execution_updates.read().await;
    println!("{result:?}");
    assert_eq!(result[0].status, JobExecutionStatus::Succeeded as i32);

    let actual: Vec<WifiProfile> = serde_json::from_str(&result[0].std_out).unwrap();
    assert_eq!(actual, expected);
}
