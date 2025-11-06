use common::{fake_connd::MockConnd, fixture::JobAgentFixture};
use orb_jobs_agent::shell::Host;
use serde_json::{self, json};
use zbus::fdo;

mod common;

#[tokio::test]
async fn it_adds_a_wifi_network() {
    // Arrange
    let fx = JobAgentFixture::new().await;

    let mut connd = MockConnd::new();
    connd.expect_add_wifi_profile().once().return_const(Ok(()));

    fx.program().shell(Host).connd(connd).spawn().await;

    let req = json!({
        "ssid": "default with with space",
        "sec": "wpa2",
        "pwd": "12345678"
    });

    // Act
    fx.enqueue_job(format!("wifi_add {req}"))
        .await
        .wait_for_completion()
        .await;

    // Assert
    let expected = json!({ "connection_success": null });

    let result = fx.execution_updates.read().await;
    let actual: serde_json::Value = serde_json::from_str(&result[0].std_out).unwrap();

    assert_eq!(expected, actual);
}

#[tokio::test]
async fn it_adds_and_connects_to_a_wifi_network() {
    // Arrange
    let fx = JobAgentFixture::new().await;

    let mut connd = MockConnd::new();
    connd.expect_add_wifi_profile().once().return_const(Ok(()));
    connd.expect_connect_to_wifi().once().return_const(Ok(()));

    fx.program().shell(Host).connd(connd).spawn().await;

    let req = json!({
        "ssid": "default wifi with space",
        "sec": "wpa3",
        "pwd": "12345678",
        "join_now": true,
    });

    // Act
    fx.enqueue_job(format!("wifi_add {req}"))
        .await
        .wait_for_completion()
        .await;

    // Assert
    let expected = json!({ "connection_success": true });

    let result = fx.execution_updates.read().await;
    let actual: serde_json::Value = serde_json::from_str(&result[0].std_out).unwrap();

    assert_eq!(expected, actual);
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
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
        "sec": "wpa2",
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
    let expected = json!({ "connection_success": false });

    let result = fx.execution_updates.read().await;
    let actual: serde_json::Value = serde_json::from_str(&result[0].std_out).unwrap();

    assert_eq!(expected, actual);
}
