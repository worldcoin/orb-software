use common::{fake_connd::MockConnd, fixture::JobAgentFixture};
use orb_jobs_agent::shell::Host;
use serde_json::{self, json};
use zbus::fdo;

mod common;

#[tokio::test]
async fn it_adds_a_wifi_network() {
    // Arrange
    let fx = JobAgentFixture::new().await;

    let join_now = false;
    let ssid = "default_wifi";
    let sec = "wpa2";
    let pwd = "12345678";
    let hidden = false;

    let expected = json!({ "connection_success": null });

    let mut connd = MockConnd::new();
    connd.expect_add_wifi_profile().once().return_const(Ok(()));

    fx.program().shell(Host).connd(connd).spawn().await;

    // Act
    fx.enqueue_job(format!("wifi_add {join_now} {ssid} {sec} {pwd} {hidden}"))
        .await
        .wait_for_completion()
        .await;

    // Assert
    let result = fx.execution_updates.read().await;
    let actual: serde_json::Value = serde_json::from_str(&result[0].std_out).unwrap();

    assert_eq!(expected, actual);
}

#[tokio::test]
async fn it_adds_and_connects_to_a_wifi_network() {
    // Arrange
    let fx = JobAgentFixture::new().await;

    let join_now = true;
    let ssid = "default_wifi";
    let sec = "wpa2";
    let pwd = "12345678";
    let hidden = false;

    let expected = json!({ "connection_success": true });

    let mut connd = MockConnd::new();
    connd.expect_add_wifi_profile().once().return_const(Ok(()));
    connd.expect_connect_to_wifi().once().return_const(Ok(()));

    fx.program().shell(Host).connd(connd).spawn().await;

    // Act
    fx.enqueue_job(format!("wifi_add {join_now} {ssid} {sec} {pwd} {hidden}"))
        .await
        .wait_for_completion()
        .await;

    // Assert
    let result = fx.execution_updates.read().await;
    let actual: serde_json::Value = serde_json::from_str(&result[0].std_out).unwrap();

    assert_eq!(expected, actual);
}

#[tokio::test]
async fn it_adds_and_fails_to_connect_to_a_wifi_network() {
    // Arrange
    let fx = JobAgentFixture::new().await;

    let join_now = true;
    let ssid = "default_wifi";
    let sec = "wpa2";
    let pwd = "12345678";
    let hidden = false;

    let expected = json!({ "connection_success": false });

    let mut connd = MockConnd::new();
    connd.expect_add_wifi_profile().once().return_const(Ok(()));
    connd
        .expect_connect_to_wifi()
        .once()
        .returning(|_| Err(fdo::Error::Failed("oh bollocks".into())));

    fx.program().shell(Host).connd(connd).spawn().await;

    // Act
    fx.enqueue_job(format!("wifi_add {join_now} {ssid} {sec} {pwd} {hidden}"))
        .await
        .wait_for_completion()
        .await;

    // Assert
    let result = fx.execution_updates.read().await;
    let actual: serde_json::Value = serde_json::from_str(&result[0].std_out).unwrap();

    assert_eq!(expected, actual);
}
