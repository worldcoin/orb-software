use common::{fake_connd::MockConnd, fixture::JobAgentFixture};
use orb_connd_dbus::{AccessPoint, AccessPointCapabilities};
use orb_jobs_agent::shell::Host;
use orb_relay_messages::jobs::v1::JobExecutionStatus;

mod common;

#[tokio::test]
async fn it_lists_wifi_profiles() {
    // Arrange
    let fx = JobAgentFixture::new().await;

    let expected = vec![
        AccessPoint {
            ssid: "apple".into(),
            bssid: "banana".into(),
            is_saved: true,
            freq_mhz: 1234,
            max_bitrate_kbps: 1234,
            strength_pct: 12,
            last_seen: "1985-04-12T23:20:50.52Z".into(),
            mode: "Ap".into(),
            capabilities: AccessPointCapabilities::default(),
            sec: "Wpa2Psk".into(),
        },
        AccessPoint {
            ssid: "pineapple".into(),
            bssid: "cherry".into(),
            is_saved: false,
            freq_mhz: 4321,
            max_bitrate_kbps: 4321,
            strength_pct: 21,
            last_seen: "1990-04-12T23:20:50.52Z".into(),
            mode: "Ap".into(),
            capabilities: AccessPointCapabilities::default(),
            sec: "Wpa3Sae".into(),
        },
    ];

    let mut connd = MockConnd::new();
    connd
        .expect_scan_wifi()
        .once()
        .return_const(Ok(expected.clone()));

    fx.program().shell(Host).connd(connd).spawn().await;

    // Act
    fx.enqueue_job("wifi_scan")
        .await
        .wait_for_completion()
        .await;

    // Assert
    let result = fx.execution_updates.read().await;
    println!("{result:?}");
    assert_eq!(result[0].status, JobExecutionStatus::Succeeded as i32);

    let actual: Vec<AccessPoint> = serde_json::from_str(&result[0].std_out).unwrap();
    assert_eq!(actual, expected);
}
