mod fixture;

use fixture::{mocks, Fixture};
use std::time::Duration;
use wiremock::{
    matchers::{method, path},
    Mock, ResponseTemplate,
};
use zbus::{fdo::DBusProxy, names::BusName};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_flushes_oes_events_to_backend() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let payload = serde_json::json!({
        "key": "value",
        "count": 42
    });
    fx.publish_oes_event("worldcoin", "test_event", payload)
        .await
        .expect("failed to publish OES event");

    // Wait for the OES flusher to pick up and flush (1s interval + buffer)
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    let oes_request = requests.iter().find(|r| {
        let body = String::from_utf8_lossy(&r.body);
        body.contains("\"oes\"")
    });
    assert!(
        oes_request.is_some(),
        "Expected a POST containing 'oes' field, got {} requests: {:?}",
        requests.len(),
        requests
            .iter()
            .map(|r| String::from_utf8_lossy(&r.body).to_string())
            .collect::<Vec<_>>()
    );

    let body = &oes_request.unwrap().body;
    let response: serde_json::Value = serde_json::from_slice(body)
        .expect("Failed to parse response body as JSON");

    let oes_events = response
        .get("oes")
        .expect("Response should contain 'oes' field")
        .as_array()
        .expect("'oes' field should be an array");

    assert_eq!(oes_events.len(), 1, "Expected exactly 1 OES event");

    let event = &oes_events[0];
    assert_eq!(
        event.get("name").and_then(|v| v.as_str()),
        Some("worldcoin/test_event"),
        "Event name should be 'worldcoin/test_event'"
    );

    assert!(
        event.get("created_at").is_some(),
        "Event should have 'created_at' timestamp"
    );

    let event_payload = event
        .get("payload")
        .expect("Event should have 'payload' field");

    let expected_payload = serde_json::json!({
        "key": "value",
        "count": 42
    });

    assert_eq!(
        event_payload, &expected_payload,
        "Event payload should match expected structure"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_exposes_a_service_in_dbus() {
    // Arrange
    let fx = Fixture::new().await;
    let dbus = DBusProxy::new(&fx.dbus).await.unwrap();
    let name =
        BusName::try_from(orb_backend_status_dbus::constants::SERVICE_NAME).unwrap();

    // Act
    fx.start().await;
    let has_owner = dbus.name_has_owner(name).await.unwrap();

    // Assert
    assert!(has_owner);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_sends_when_connected_with_token() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_millis(100)).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        !requests.is_empty(),
        "Expected HTTP requests when connected with token"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_does_not_send_when_disconnected() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_millis(50)).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        requests.is_empty(),
        "Expected NO requests when disconnected, got {}",
        requests.len()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_does_not_send_when_no_token() {
    // Arrange
    let fx = Fixture::spawn_without_token(Duration::from_millis(50)).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        requests.is_empty(),
        "Expected NO requests without token, got {}",
        requests.len()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_does_not_send_when_nothing_available() {
    // Arrange
    let fx = Fixture::spawn_without_token(Duration::from_millis(50)).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        requests.is_empty(),
        "Expected NO requests without token or connectivity, got {}",
        requests.len()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_sends_periodically() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_millis(50)).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        requests.len() >= 3,
        "Expected at least 3 periodic sends, got {}",
        requests.len()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_sends_immediately_on_update_rebooting() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let before = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(before.is_empty(), "Should not have sent (disconnected)");
    fx.set_connected().await.expect("failed to set connected");
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger rebooting");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let after = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        !after.is_empty(),
        "Expected send after Rebooting state + connectivity"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_sends_immediately_on_ssid_change() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    fx.set_connected_with_ssid("HomeWifi")
        .await
        .expect("failed to set connected with HomeWifi");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let initial_count = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();
    fx.set_connected_with_ssid("OfficeWifi")
        .await
        .expect("failed to change SSID to OfficeWifi");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let after = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        after.len() > initial_count,
        "Expected immediate send on SSID change, got {} (was {})",
        after.len(),
        initial_count
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_does_not_send_urgnet_when_disconnected() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger rebooting");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        requests.is_empty(),
        "Should not send urgent when disconnected"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_waits_for_connectivity_before_urgent_send() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger rebooting");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let before = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(before.is_empty(), "Should not send while disconnected");
    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let after = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        !after.is_empty(),
        "Expected send after connectivity restored with urgent flag"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_sends_after_connectivity_restored() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_millis(100)).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let before = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(before.is_empty(), "Should not send when disconnected");
    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let after = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        !after.is_empty(),
        "Expected send after connectivity restored"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_retries_on_backend_error() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_millis(50)).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        requests.len() >= 2,
        "Expected at least 2 retry attempts, got {}",
        requests.len()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_recovers_after_backend_comes_back() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_millis(50)).await;
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
    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(
        requests.len() >= 3,
        "Expected at least 3 requests (2 failures + 1 success), got {}",
        requests.len()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_includes_update_progress_in_payload() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(100)).await;
    mocks::provide_update_progress(&fx.dbus, mocks::UpdateAgentState::Downloading, 50)
        .await
        .expect("failed to provide update progress");
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger send");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(!requests.is_empty(), "Expected HTTP request");
    let body = String::from_utf8_lossy(&requests.last().unwrap().body);
    assert!(
        body.contains("update_progress") || body.contains("Rebooting"),
        "Expected update_progress in payload, got: {}",
        body
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_includes_signup_state_in_payload() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(100)).await;
    mocks::provide_signup_state(&fx.dbus, mocks::SignupState::InProgress)
        .await
        .expect("failed to provide signup state");
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_includes_cellular_status_in_payload() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;

    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(100)).await;

    mocks::provide_cellular_status(&fx.dbus, "123456789012345", Some("T-Mobile"))
        .await
        .expect("failed to provide cellular status");

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_includes_connd_report_in_payload() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;

    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(100)).await;

    mocks::provide_connd_report(&fx.dbus, Some("TestNetwork"))
        .await
        .expect("failed to provide connd report");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(!requests.is_empty(), "Expected HTTP request");

    let body = String::from_utf8_lossy(&requests.last().unwrap().body);
    assert!(
        body.contains("wifi") || body.contains("TestNetwork") || body.contains("connd"),
        "Expected wifi/connd data in payload, got: {}",
        body
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_stops_cleanly_on_shutdown() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_millis(50)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    let handle = fx.start().await;

    fx.set_connected().await.expect("failed to set connected");

    tokio::time::sleep(Duration::from_millis(100)).await;

    fx.stop();

    let result = tokio::time::timeout(Duration::from_secs(2), handle).await;

    // Assert
    assert!(result.is_ok(), "Task should complete on shutdown");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_handles_connectivity_flapping() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_millis(100)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let initial_count = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();
    assert!(initial_count >= 1, "Should send while connected");

    fx.set_disconnected()
        .await
        .expect("failed to set disconnected");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let disconnected_count = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let reconnected_count = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    // Assert
    assert!(
        reconnected_count > disconnected_count,
        "Should resume sending after reconnect, got {} (was {})",
        reconnected_count,
        disconnected_count
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_handles_multiple_urgent_triggers() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;

    fx.set_connected_with_ssid("Network1")
        .await
        .expect("failed to set connected with Network1");
    tokio::time::sleep(Duration::from_millis(150)).await;

    let after_first = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    fx.set_connected_with_ssid("Network2")
        .await
        .expect("failed to change SSID to Network2");
    tokio::time::sleep(Duration::from_millis(150)).await;

    let after_second = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

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

    // Assert
    assert!(
        after_second > after_first,
        "Second urgent should trigger send"
    );
    assert!(
        after_third > after_second,
        "Third urgent should trigger send"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_stores_urgent_flag_on_connectivity_loss() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger rebooting");
    tokio::time::sleep(Duration::from_millis(100)).await;

    let before = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();
    assert_eq!(before, 0, "Should not send while disconnected");

    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(200)).await;

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

    fx.set_disconnected()
        .await
        .expect("failed to set disconnected");
    tokio::time::sleep(Duration::from_millis(200)).await;

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

    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let after_reconnect = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    // Assert
    assert!(
        after_reconnect > before_reconnect,
        "Second urgent should also persist and send after reconnect"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_sends_after_token_becomes_available() {
    // Arrange
    let fx = Fixture::spawn_without_token(Duration::from_millis(100)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;

    // Set connected state after program starts
    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let before = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();
    assert_eq!(before, 0, "Should not send without token");

    fx.token_mock
        .as_ref()
        .unwrap()
        .update_token("new-auth-token")
        .await
        .expect("failed to update token");

    tokio::time::sleep(Duration::from_millis(300)).await;

    // Assert
    let after = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();
    assert!(after >= 1, "Expected send after token became available");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_stops_sending_when_token_revoked() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_millis(100)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;

    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(250)).await;

    let before_revoke = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();
    assert!(before_revoke >= 1, "Should send with token");

    fx.token_mock
        .as_ref()
        .unwrap()
        .update_token("")
        .await
        .expect("failed to revoke token");

    tokio::time::sleep(Duration::from_millis(300)).await;

    let after_revoke = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    tokio::time::sleep(Duration::from_millis(300)).await;

    let final_count = fx
        .mock_server
        .received_requests()
        .await
        .unwrap_or_default()
        .len();

    // Assert
    assert_eq!(
        after_revoke, final_count,
        "Should stop sending after token revoked"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_includes_hardware_states_in_payload() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;

    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(100)).await;

    fx.publish_hardware_state("pwr_supply", "success", "corded")
        .await
        .expect("failed to publish hardware state");

    // Trigger an immediate send
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger send");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(!requests.is_empty(), "Expected HTTP request");

    let body = String::from_utf8_lossy(&requests.last().unwrap().body);
    assert!(
        body.contains("hardware_states") && body.contains("pwr_supply"),
        "Expected hardware_states with pwr_supply in payload, got: {}",
        body
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_includes_multiple_hardware_states_in_payload() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;

    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Publish multiple hardware states
    fx.publish_hardware_state("pwr_supply", "success", "corded")
        .await
        .expect("failed to publish pwr_supply state");
    fx.publish_hardware_state("battery", "success", "charging")
        .await
        .expect("failed to publish battery state");
    fx.publish_hardware_state("main_mcu", "failure", "disconnected")
        .await
        .expect("failed to publish main_mcu state");

    // Trigger an immediate send
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger send");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(!requests.is_empty(), "Expected HTTP request");

    let body = String::from_utf8_lossy(&requests.last().unwrap().body);
    assert!(
        body.contains("hardware_states"),
        "Expected hardware_states in payload, got: {}",
        body
    );
    assert!(
        body.contains("pwr_supply"),
        "Expected pwr_supply in payload, got: {}",
        body
    );
    assert!(
        body.contains("battery"),
        "Expected battery in payload, got: {}",
        body
    );
    assert!(
        body.contains("main_mcu"),
        "Expected main_mcu in payload, got: {}",
        body
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_updates_hardware_state_on_change() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;

    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Initial state
    fx.publish_hardware_state("pwr_supply", "success", "corded")
        .await
        .expect("failed to publish initial state");

    // Trigger send
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger send");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    let first_body = String::from_utf8_lossy(&requests.last().unwrap().body);
    assert!(
        first_body.contains("corded"),
        "Expected 'corded' in first payload"
    );

    // Update state
    fx.publish_hardware_state("pwr_supply", "success", "battery")
        .await
        .expect("failed to publish updated state");

    // Trigger another send
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger second send");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    let last_body = String::from_utf8_lossy(&requests.last().unwrap().body);
    assert!(
        last_body.contains("battery"),
        "Expected 'battery' in updated payload, got: {}",
        last_body
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_includes_front_als_in_payload() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;

    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(100)).await;

    fx.publish_front_als(150, 0) // 0 = ALS_OK
        .await
        .expect("failed to publish front_als");

    // Trigger an immediate send
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger send");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(!requests.is_empty(), "Expected HTTP request");

    let body = String::from_utf8_lossy(&requests.last().unwrap().body);
    assert!(
        body.contains("main_mcu") && body.contains("front_als"),
        "Expected main_mcu with front_als in payload, got: {}",
        body
    );
    assert!(
        body.contains("ambient_light_lux") && body.contains("150"),
        "Expected ambient_light_lux: 150 in payload, got: {}",
        body
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_includes_front_als_with_err_range_flag() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;

    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Publish with ALS_ERR_RANGE flag (too much light)
    fx.publish_front_als(500, 1) // 1 = ALS_ERR_RANGE
        .await
        .expect("failed to publish front_als");

    // Trigger an immediate send
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger send");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    assert!(!requests.is_empty(), "Expected HTTP request");

    let body = String::from_utf8_lossy(&requests.last().unwrap().body);
    assert!(
        body.contains("err_range"),
        "Expected 'err_range' flag in payload, got: {}",
        body
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_updates_front_als_on_change() {
    // Arrange
    let fx = Fixture::spawn_with_token(Duration::from_secs(60)).await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&fx.mock_server)
        .await;

    // Act
    fx.start().await;

    fx.set_connected().await.expect("failed to set connected");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Initial value
    fx.publish_front_als(100, 0) // 0 = ALS_OK
        .await
        .expect("failed to publish initial front_als");

    // Trigger send
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger send");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    let first_body = String::from_utf8_lossy(&requests.last().unwrap().body);
    assert!(
        first_body.contains("100"),
        "Expected '100' lux in first payload"
    );

    // Update value
    fx.publish_front_als(250, 0) // 0 = ALS_OK
        .await
        .expect("failed to publish updated front_als");

    // Trigger another send
    mocks::trigger_update_progress_rebooting(&fx.dbus)
        .await
        .expect("failed to trigger second send");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Assert
    let requests = fx.mock_server.received_requests().await.unwrap_or_default();
    let last_body = String::from_utf8_lossy(&requests.last().unwrap().body);
    assert!(
        last_body.contains("250"),
        "Expected '250' lux in updated payload, got: {}",
        last_body
    );
}
