use chrono::Utc;
use common::{fake_connd::MockConnd, fixture::JobAgentFixture};
use mockall::predicate::eq;
use orb_connd_dbus::{AccessPoint, AccessPointCapabilities, ConnectionState};
use orb_jobs_agent::shell::Host;
use serde_json::{self, json};
use zbus::fdo;

mod common;

#[tokio::test(flavor = "multi_thread")]
async fn it_adds_a_wifi_network() {
    // Arrange
    let fx = JobAgentFixture::new().await;

    let mut connd = MockConnd::new();
    connd.expect_add_wifi_profile().once().return_const(Ok(()));

    fx.program().shell(Host).connd(connd).spawn().await;

    let req = json!({
        "ssid": "default with with space",
        "sec": "Wpa2Psk",
        "pwd": "12345678"
    });

    // Act
    fx.enqueue_job(format!("wifi_add {req}"))
        .await
        .wait_for_completion()
        .await;

    // Assert
    let expected = json!({ "connection_success": null, "network": null });

    let result = fx.execution_updates.read().await;
    let actual: serde_json::Value = serde_json::from_str(&result[0].std_out).unwrap();

    assert_eq!(expected, actual);
}

#[tokio::test(flavor = "multi_thread")]
async fn it_adds_and_connects_to_a_wifi_network() {
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
    connd.expect_add_wifi_profile().once().return_const(Ok(()));
    connd
        .expect_connect_to_wifi()
        .with(eq("default wifi with space".to_string()))
        .once()
        .returning(move |_| Ok(returning.clone()));
    connd
        .expect_connection_state()
        .return_const(Ok(ConnectionState::Connected));

    fx.program().shell(Host).connd(connd).spawn().await;

    let req = json!({
        "ssid": "default wifi with space",
        "sec": "Wpa3Sae",
        "pwd": "12345678",
        "join_now": true,
    });

    // Act
    fx.enqueue_job(format!("wifi_add {req}"))
        .await
        .wait_for_completion()
        .await;

    // Assert
    let expected = json!({ "connection_success": true, "network": expected.clone() });

    let result = fx.execution_updates.read().await;
    let actual: serde_json::Value = serde_json::from_str(&result[0].std_out).unwrap();

    assert_eq!(expected, actual);
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test(flavor = "multi_thread")]
async fn it_adds_and_fails_to_connect_to_a_wifi_network() {
    // Arrange
    let fx = JobAgentFixture::new().await;

    let mut connd = MockConnd::new();
    connd.expect_add_wifi_profile().once().return_const(Ok(()));
    connd
        .expect_connect_to_wifi()
        .once()
        .returning(|_| Err(fdo::Error::Failed("oh bollocks".into())));

    fx.program().shell(Host).connd(connd).spawn().await;

    let req = json!({
        "ssid": "default wifi with space",
        "sec": "Wpa2Psk",
        "pwd": "12345678",
        "join_now": true,
        "hidden": false,
    });

    // Act
    fx.enqueue_job(format!("wifi_add {req}"))
        .await
        .wait_for_completion()
        .await;

    // Assert
    let expected = json!({ "connection_success": false, "network": null });

    let result = fx.execution_updates.read().await;
    let actual: serde_json::Value = serde_json::from_str(&result[0].std_out).unwrap();

    assert_eq!(expected, actual);
}
