mod fixture;

use fixture::{mocks, Fixture};
use std::time::Duration;
use wiremock::{
    matchers::{method, path},
    Mock, ResponseTemplate,
};
use zbus::{fdo::DBusProxy, names::BusName};

#[tokio::test]
async fn it_exposes_a_service_in_dbus() {
    let fx = Fixture::new().await;

    let dbus = DBusProxy::new(&fx.dbus).await.unwrap();
    let name =
        BusName::try_from(orb_backend_status_dbus::constants::SERVICE_NAME).unwrap();

    fx.start().await;
    let has_owner = dbus.name_has_owner(name).await.unwrap();

    assert!(has_owner);
}

#[tokio::test]
async fn it_sends_when_connected_with_token() {
    // Arrange - happy path: connected + token
    let fx = Fixture::spawn_connected_with_token(Duration::from_millis(100)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Assert - should have sent
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        !requests.is_empty(),
        "Expected HTTP requests when connected with token"
    );
}

#[tokio::test]
async fn it_does_not_send_when_disconnected() {
    // Arrange - has token but offline
    let fx = Fixture::spawn_disconnected_with_token(Duration::from_millis(50)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert - should NOT have sent
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        requests.is_empty(),
        "Expected NO requests when disconnected, got {}",
        requests.len()
    );
}

#[tokio::test]
async fn it_does_not_send_when_no_token() {
    // Arrange - connected but no token
    let fx = Fixture::spawn_connected_without_token(Duration::from_millis(50)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert - should NOT have sent
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        requests.is_empty(),
        "Expected NO requests without token, got {}",
        requests.len()
    );
}

#[tokio::test]
async fn it_does_not_send_when_nothing_available() {
    // Arrange - no token, no connectivity
    let fx = Fixture::spawn_disconnected_without_token(Duration::from_millis(50)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert - should NOT have sent
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        requests.is_empty(),
        "Expected NO requests without token or connectivity, got {}",
        requests.len()
    );
}

#[tokio::test]
async fn it_sends_periodically() {
    // Arrange - short interval to verify periodic behavior
    let fx = Fixture::spawn_connected_with_token(Duration::from_millis(50)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Assert - should have sent multiple times
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        requests.len() >= 3,
        "Expected at least 3 periodic sends, got {}",
        requests.len()
    );
}

#[tokio::test]
async fn it_sends_immediately_on_update_rebooting() {
    // Arrange - long interval so we can distinguish urgent from periodic
    // Start disconnected so no initial send happens
    let fx = Fixture::spawn_disconnected_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Verify no sends yet (disconnected)
    let before = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(before.is_empty(), "Should not have sent (disconnected)");

    // Trigger urgent: UpdateProgress with Rebooting state
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger rebooting");

    // Connect
    fx.connd_mock.as_ref().unwrap().set_connected();

    // Wait for connectivity poll + buffer
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Assert - should have sent after urgent + connectivity
    let after = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        !after.is_empty(),
        "Expected send after Rebooting state + connectivity"
    );
}

#[tokio::test]
async fn it_sends_immediately_on_ssid_change() {
    // Arrange - long interval so any send must be from urgent trigger
    let fx = Fixture::spawn_connected_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // First, set an initial SSID
    mocks::provide_connd_report(&fx.dbus, Some("HomeWifi"))
        .await
        .expect("failed to provide initial connd report");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Clear any sends from the initial connd report
    let initial_count = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    // Now change SSID - this should trigger urgent
    mocks::provide_connd_report(&fx.dbus, Some("OfficeWifi"))
        .await
        .expect("failed to provide connd report with new SSID");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert - should have sent immediately on SSID change
    let after = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        after.len() > initial_count,
        "Expected immediate send on SSID change, got {} (was {})",
        after.len(),
        initial_count
    );
}

#[tokio::test]
async fn it_urgent_does_not_send_when_disconnected() {
    // Arrange - urgent but disconnected
    let fx = Fixture::spawn_disconnected_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Trigger urgent
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger rebooting");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert - should NOT send (disconnected)
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        requests.is_empty(),
        "Should not send urgent when disconnected"
    );
}

#[tokio::test]
async fn it_urgent_waits_for_connectivity_then_sends() {
    // Arrange - start disconnected with urgent pending
    let fx = Fixture::spawn_disconnected_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Trigger urgent while disconnected
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger rebooting");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Verify no send yet
    let before = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(before.is_empty(), "Should not send while disconnected");

    // Restore connectivity
    fx.connd_mock.as_ref().unwrap().set_connected();

    // Wait for connectivity poll + buffer
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Assert - should have sent after connectivity restored
    let after = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        !after.is_empty(),
        "Expected send after connectivity restored with urgent flag"
    );
}

#[tokio::test]
async fn it_sends_after_connectivity_restored() {
    // Arrange - start disconnected, short sender interval but connectivity polls every 2s
    let fx = Fixture::spawn_disconnected_with_token(Duration::from_millis(100)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Verify no send yet
    let before = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(before.is_empty(), "Should not send when disconnected");

    // Connectivity restored
    fx.connd_mock.as_ref().unwrap().set_connected();

    // Wait for connectivity poll + buffer
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Assert - should have sent after connectivity restored
    let after = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        !after.is_empty(),
        "Expected send after connectivity restored"
    );
}

#[tokio::test]
async fn it_retries_on_backend_error() {
    // Arrange
    let fx = Fixture::spawn_connected_with_token(Duration::from_millis(50)).await;

    // Backend returns 500 error
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Assert - should have retried multiple times
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        requests.len() >= 2,
        "Expected at least 2 retry attempts, got {}",
        requests.len()
    );
}

#[tokio::test]
async fn backoff_limits_retry_rate() {
    // Arrange - with explicit backoff config
    let mut fx = Fixture::with()
        .sender_interval(Duration::from_millis(50))
        .sender_min_backoff(Duration::from_millis(200))
        .sender_max_backoff(Duration::from_millis(400))
        .build()
        .await;

    fx.setup_mocks_connected_with_token().await;

    // Backend returns 500 error
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(800)).await;

    // Assert - backoff should limit the number of retries
    // Without backoff at 50ms interval, we'd get ~16 attempts in 800ms
    // With backoff (200ms min, 400ms max), we should get significantly fewer
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        requests.len() >= 2 && requests.len() <= 10,
        "Expected 2-12 attempts with backoff (fewer than ~16 without), got {}",
        requests.len()
    );
}

#[tokio::test]
async fn it_recovers_after_backend_comes_back() {
    // Arrange
    let fx = Fixture::spawn_connected_with_token(Duration::from_millis(50)).await;

    // First 2 requests fail, then succeed
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(2)
        .mount(&fx.mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Assert - should have made requests (2 failures + at least 1 success)
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        requests.len() >= 3,
        "Expected at least 3 requests (2 failures + 1 success), got {}",
        requests.len()
    );
}

#[tokio::test]
async fn it_includes_update_progress_in_payload() {
    // Arrange
    let fx = Fixture::spawn_connected_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Provide update progress
    mocks::provide_update_progress(&fx.dbus, mocks::UpdateAgentState::Downloading, 50)
        .await
        .expect("failed to provide update progress");

    // Trigger urgent to force send
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger send");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert - verify request was made and contains data
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(!requests.is_empty(), "Expected HTTP request");

    // Check that the body contains update progress data
    let body = String::from_utf8_lossy(&requests.last().unwrap().body);
    assert!(
        body.contains("update_progress") || body.contains("Rebooting"),
        "Expected update_progress in payload, got: {}",
        body
    );
}

#[tokio::test]
async fn it_includes_signup_state_in_payload() {
    // Arrange
    let fx = Fixture::spawn_connected_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Provide signup state
    mocks::provide_signup_state(&fx.dbus, mocks::SignupState::InProgress)
        .await
        .expect("failed to provide signup state");

    // Trigger urgent to force send
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger send");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(!requests.is_empty(), "Expected HTTP request");

    let body = String::from_utf8_lossy(&requests.last().unwrap().body);
    assert!(
        body.contains("signup_state") || body.contains("InProgress"),
        "Expected signup_state in payload, got: {}",
        body
    );
}

#[tokio::test]
async fn it_includes_cellular_status_in_payload() {
    // Arrange
    let fx = Fixture::spawn_connected_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Provide cellular status
    mocks::provide_cellular_status(&fx.dbus, "123456789012345", Some("T-Mobile"))
        .await
        .expect("failed to provide cellular status");

    // Trigger urgent to force send
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger send");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(!requests.is_empty(), "Expected HTTP request");

    let body = String::from_utf8_lossy(&requests.last().unwrap().body);
    assert!(
        body.contains("cellular") || body.contains("123456789012345"),
        "Expected cellular data in payload, got: {}",
        body
    );
}

#[tokio::test]
async fn it_includes_connd_report_in_payload() {
    // Arrange
    let fx = Fixture::spawn_connected_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    mocks::provide_connd_report(&fx.dbus, Some("TestNetwork"))
        .await
        .expect("failed to provide connd report");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(!requests.is_empty(), "Expected HTTP request");

    let body = String::from_utf8_lossy(&requests.last().unwrap().body);
    // Should contain wifi/network info
    assert!(
        body.contains("wifi") || body.contains("TestNetwork") || body.contains("connd"),
        "Expected wifi/connd data in payload, got: {}",
        body
    );
}

#[tokio::test]
async fn it_stops_cleanly_on_shutdown() {
    // Arrange
    let fx = Fixture::spawn_connected_with_token(Duration::from_millis(50)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    let handle = fx.start().await;

    // Let it run briefly
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Signal shutdown
    fx.stop();

    // Wait for task to complete
    let result = tokio::time::timeout(Duration::from_secs(2), handle).await;

    // Assert - should complete without panic
    assert!(result.is_ok(), "Task should complete on shutdown");
}

#[tokio::test]
async fn it_stops_sending_when_connectivity_lost() {
    // Arrange - start connected
    let fx = Fixture::spawn_connected_with_token(Duration::from_millis(50)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act - let it send a few times
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let before_disconnect = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();
    assert!(before_disconnect >= 1, "Should have sent while connected");

    // Disconnect
    fx.connd_mock.as_ref().unwrap().set_disconnected();

    // Wait for connectivity poll to detect disconnect
    tokio::time::sleep(Duration::from_millis(300)).await;

    let after_disconnect = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    // Wait more and verify no new sends
    tokio::time::sleep(Duration::from_millis(200)).await;

    let final_count = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    // Should have stopped sending after disconnect
    assert_eq!(
        after_disconnect, final_count,
        "Should stop sending after disconnect"
    );
}

#[tokio::test]
async fn it_handles_connectivity_flapping() {
    // Arrange - start connected
    let fx = Fixture::spawn_connected_with_token(Duration::from_millis(100)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    fx.start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let initial_count = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();
    assert!(initial_count >= 1, "Should send while connected");

    // Disconnect
    fx.connd_mock.as_ref().unwrap().set_disconnected();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let disconnected_count = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    // Reconnect
    fx.connd_mock.as_ref().unwrap().set_connected();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let reconnected_count = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    // Should resume sending after reconnect
    assert!(
        reconnected_count > disconnected_count,
        "Should resume sending after reconnect, got {} (was {})",
        reconnected_count,
        disconnected_count
    );
}

#[tokio::test]
async fn it_handles_multiple_urgent_triggers() {
    // Arrange - long interval so sends are only from urgent
    let fx = Fixture::spawn_connected_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    fx.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // First urgent: SSID change
    mocks::provide_connd_report(&fx.dbus, Some("Network1"))
        .await
        .expect("failed to provide connd report");
    tokio::time::sleep(Duration::from_millis(150)).await;

    let after_first = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    // Second urgent: another SSID change
    mocks::provide_connd_report(&fx.dbus, Some("Network2"))
        .await
        .expect("failed to provide connd report");
    tokio::time::sleep(Duration::from_millis(150)).await;

    let after_second = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    // Third urgent: reboot notification
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger rebooting");
    tokio::time::sleep(Duration::from_millis(150)).await;

    let after_third = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    // Each urgent trigger should cause a send
    assert!(
        after_second > after_first,
        "Second urgent should trigger send"
    );
    assert!(
        after_third > after_second,
        "Third urgent should trigger send"
    );
}

#[tokio::test]
async fn it_urgent_flag_persists_across_connectivity_loss() {
    // Arrange - start disconnected
    let fx = Fixture::spawn_disconnected_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    fx.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Trigger urgent while disconnected
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger rebooting");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify no send yet
    let before = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();
    assert_eq!(before, 0, "Should not send while disconnected");

    // Connect
    fx.connd_mock.as_ref().unwrap().set_connected();
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Verify urgent send happened
    let after_connect = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();
    assert!(
        after_connect >= 1,
        "Urgent flag should persist and trigger send after connect"
    );

    // Disconnect again
    fx.connd_mock.as_ref().unwrap().set_disconnected();
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Trigger another urgent while disconnected
    mocks::provide_connd_report(&fx.dbus, Some("NewNetwork"))
        .await
        .expect("failed to provide connd report");
    tokio::time::sleep(Duration::from_millis(100)).await;

    let before_reconnect = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    // Reconnect
    fx.connd_mock.as_ref().unwrap().set_connected();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let after_reconnect = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    assert!(
        after_reconnect > before_reconnect,
        "Second urgent should also persist and send after reconnect"
    );
}

#[tokio::test]
async fn it_includes_core_stats_in_payload() {
    // Arrange
    let fx = Fixture::spawn_connected_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    fx.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Provide core stats
    mocks::provide_core_stats(&fx.dbus, 85.5, 42.0)
        .await
        .expect("failed to provide core stats");

    // Trigger urgent to force send
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger send");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(!requests.is_empty(), "Expected HTTP request");

    let body = String::from_utf8_lossy(&requests.last().unwrap().body);
    // Should contain battery or temperature data
    assert!(
        body.contains("battery") || body.contains("temperature") || body.contains("85"),
        "Expected core stats in payload, got: {}",
        body
    );
}

#[tokio::test]
async fn it_includes_net_stats_in_payload() {
    // Arrange
    let fx = Fixture::spawn_connected_with_token(Duration::from_millis(100)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    fx.start().await;

    // Wait for net stats collector to run (polls at netstats_poll_interval)
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Trigger a send
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger send");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(!requests.is_empty(), "Expected HTTP request");

    let body = String::from_utf8_lossy(&requests.last().unwrap().body);
    // Should contain network interface data
    assert!(
        body.contains("net_stats")
            || body.contains("wlan")
            || body.contains("interfaces"),
        "Expected net stats in payload, got: {}",
        body
    );
}

#[tokio::test]
async fn it_sends_after_token_becomes_available() {
    // Arrange - start without token
    let fx = Fixture::spawn_connected_without_token(Duration::from_millis(100)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Verify no send yet
    let before = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();
    assert_eq!(before, 0, "Should not send without token");

    // Token becomes available (with D-Bus signal)
    fx.token_mock
        .as_ref()
        .unwrap()
        .update_token("new-auth-token")
        .await
        .expect("failed to update token");

    // Wait for token change to propagate
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Assert - should have sent after token became available
    let after = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();
    assert!(after >= 1, "Expected send after token became available");
}

#[tokio::test]
async fn it_stops_sending_when_token_revoked() {
    // Arrange - start with token
    let fx = Fixture::spawn_connected_with_token(Duration::from_millis(100)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    fx.start().await;
    tokio::time::sleep(Duration::from_millis(250)).await;

    let before_revoke = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();
    assert!(before_revoke >= 1, "Should send with token");

    // Revoke token (set to empty, with D-Bus signal)
    fx.token_mock
        .as_ref()
        .unwrap()
        .update_token("")
        .await
        .expect("failed to revoke token");

    // Wait for token change to propagate
    tokio::time::sleep(Duration::from_millis(300)).await;

    let after_revoke = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    // Wait more and verify no new sends
    tokio::time::sleep(Duration::from_millis(300)).await;

    let final_count = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    assert_eq!(
        after_revoke, final_count,
        "Should stop sending after token revoked"
    );
}
