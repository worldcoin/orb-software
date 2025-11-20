use chrono::Utc;
use common::{fake_connd::MockConnd, fixture::JobAgentFixture};
use mockall::predicate::eq;
use orb_connd_dbus::{AccessPoint, AccessPointCapabilities, ConnectionState};
use orb_jobs_agent::shell::Host;
use orb_relay_messages::jobs::v1::JobExecutionStatus;

mod common;

#[tokio::test]
async fn it_connects_to_a_wifi_network() {
    // Arrange
    let fx = JobAgentFixture::new().await;

    let expected = AccessPoint {
        ssid: "bla".into(),
        bssid: "ble".into(),
        is_saved: true,
        freq_mhz: 1234,
        max_bitrate_kbps: 1234,
        strength_pct: 12,
        last_seen: Utc::now().to_rfc3339(),
        mode: "idk".into(),
        capabilities: AccessPointCapabilities::default(),
        sec: "Wpa2Psk".into(),
        is_active: true,
    };

    let returning = expected.clone();
    let mut connd = MockConnd::new();
    connd
        .expect_connect_to_wifi()
        .with(eq("ssid name with space".to_string()))
        .once()
        .returning(move |_| Ok(returning.clone()));

    connd
        .expect_connection_state()
        .return_const(Ok(ConnectionState::Connected));

    fx.program().shell(Host).connd(connd).spawn().await;

    // Act
    fx.enqueue_job("wifi_connect ssid name with space")
        .await
        .wait_for_completion()
        .await;

    // Assert
    let result = fx.execution_updates.read().await;
    let actual: AccessPoint = serde_json::from_str(&result[0].std_out)
        .unwrap_or_else(|_| panic!("accesspoint in {:?}", result[0]));

    assert_eq!(
        result[0].status,
        JobExecutionStatus::Succeeded as i32,
        "{:?}",
        result[0]
    );

    assert_eq!(actual, expected, "{:?}", result[0]);
}
