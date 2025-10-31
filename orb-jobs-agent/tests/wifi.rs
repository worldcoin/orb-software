use async_tempfile::TempFile;
use common::fixture::JobAgentFixture;
use orb_jobs_agent::shell::Host;
use serde_json::{self, json};
use tokio::fs;

mod common;

#[tokio::test]
async fn it_adds_a_wifi_network() {
    // Arrange
    let fx = JobAgentFixture::new().await;

    let join_now = "false";
    let ssid = "default_wifi";
    let sec = "wpa2";
    let pwd = "12345678";
    let hidden = "false";

    let expected = json!({"profile_added": true, "joined_network": false});

    fx.spawn_program(Host);

    // Act
    fx.enqueue_job(format!("wifi_add {join_now} {ssid} {sec} {pwd} {hidden}"))
        .await
        .wait_for_completion()
        .await;

    // Assert
    let result = fx.execution_updates.read().await;
    println!("{:?}", result);
    let actual: serde_json::Value = serde_json::from_str(&result[0].std_out).unwrap();

    assert_eq!(expected, actual);
}

#[tokio::test]
async fn it_adds_and_connects_to_a_wifi_network() {}

#[tokio::test]
async fn it_adds_and_fails_to_connect_to_a_wifi_network() {}
