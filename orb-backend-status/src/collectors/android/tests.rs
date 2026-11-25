use super::*;
use crate::{
    backend::client::{Err, StatusClient},
    orb_event_stream::{Event, Payload},
};
use futures_util::FutureExt;
use orb_dogd::test::MetricSinkhole;
use orb_info::{orb_id::test_orb_id, orb_name::test_orb_name};
use secrecy::ExposeSecret;
use std::time::Duration;
use tokio::time::timeout;
use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

async fn setup(token: &str, server: &MockServer) -> (Collectors, StatusClient) {
    let (collectors, token_rx, connectivity_rx) = Collectors::new(
        Config {
            token: SecretString::new(token.to_owned()),
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();

    let client = StatusClient::builder()
        .metrics(MetricSinkhole)
        .orb_id(test_orb_id())
        .orb_name(test_orb_name())
        .jabil_id("android-test".parse().unwrap())
        .orb_os_version("android-test".to_owned())
        .endpoint(server.uri().parse().unwrap())
        .req_timeout(Duration::from_millis(100))
        .min_req_retry_interval(Duration::from_millis(1))
        .max_req_retry_interval(Duration::from_millis(2))
        .attest_token_rx(token_rx)
        .connectivity_rx(connectivity_rx)
        .build();

    (collectors, client)
}

#[tokio::test]
async fn static_channels_stay_open_without_background_reporters() {
    let shutdown = CancellationToken::new();
    let (collectors, token_rx, connectivity_rx) = Collectors::new(
        Config {
            token: SecretString::new("static-token".to_owned()),
        },
        shutdown.clone(),
    )
    .await
    .unwrap();

    assert_eq!(token_rx.borrow().expose_secret(), "static-token");
    assert!(!connectivity_rx.borrow().is_connected());
    assert!(!token_rx.has_changed().unwrap());
    assert!(!connectivity_rx.has_changed().unwrap());
    assert!(collectors
        .spawn_reporters(PathBuf::new(), shutdown)
        .is_empty());
    assert!(
        timeout(Duration::from_millis(10), collectors.wait_for_urgent_send())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn wifi_association_changes_wake_the_online_sender() {
    use serde_json::json;

    let server = MockServer::start().await;
    let (collectors, client) = setup("static-token", &server).await;
    let shutdown = CancellationToken::new();
    let oes = OrbEventStream::start(client, shutdown.clone());

    for (ssid, rssi, urgent) in [
        (Some("Lab"), -50, true),
        (Some("Lab"), -70, false),
        (Some("Lab"), -70, false),
        (Some("Guest"), -60, true),
        (None, 0, false),
        (Some(""), -65, true), // Associated, but Android redacted the SSID.
        (None, 0, false),
    ] {
        let connections: Vec<_> = ssid
            .into_iter()
            .map(|name| {
                json!({
                    "iface": "WiFi", "name": name, "primary": true,
                    "has_internet": true, "rssi_dbm": rssi,
                })
            })
            .collect();
        let payload = Payload {
            headers: oes::Headers::default().mode(oes::Mode::CacheOnly),
            event: Event {
                name: "raven/active_connections".into(),
                created_at: 123,
                payload: Some(
                    json!({"connections": connections, "connectivity_uri": ""}),
                ),
            },
        };
        // Normal reports are forwarded without changing the cached association.
        ingest(
            &oes,
            &collectors.state,
            Payload {
                headers: oes::Headers::default().mode(oes::Mode::Normal),
                event: payload.event.clone(),
            },
        )
        .unwrap();
        assert!(collectors.wait_for_urgent_send().now_or_never().is_none());
        ingest(&oes, &collectors.state, payload).unwrap();
        assert_eq!(
            collectors.wait_for_urgent_send().now_or_never().is_some(),
            urgent
        );
        let wifi = wifi_report(&oes.cached().unwrap());
        assert_eq!(
            wifi.as_ref().and_then(|w| w.ssid.as_deref()),
            ssid.filter(|s| !s.is_empty())
        );
        assert_eq!(
            wifi.and_then(|w| w.quality?.signal_level),
            ssid.map(|_| rssi)
        );
    }
    shutdown.cancel();
}

#[tokio::test]
async fn http_waits_for_a_validated_default_network_and_resumes_on_recovery() {
    use serde_json::json;

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(202))
        .expect(2)
        .mount(&server)
        .await;
    let (collectors, client) = setup("static-token", &server).await;
    let shutdown = CancellationToken::new();
    let oes = OrbEventStream::start(client.clone(), shutdown.clone());
    let update = |connections: serde_json::Value| {
        ingest(
            &oes,
            &collectors.state,
            Payload {
                headers: oes::Headers::default().mode(oes::Mode::CacheOnly),
                event: Event {
                    name: "raven/active_connections".into(),
                    created_at: 0,
                    payload: Some(
                        json!({"connections": connections, "connectivity_uri": ""}),
                    ),
                },
            },
        )
        .unwrap();
    };
    let wifi = |primary: bool, validated: bool| {
        json!({
            "iface": "WiFi", "name": "Lab", "primary": primary, "has_internet": validated
        })
    };

    assert!(matches!(
        client.req(Default::default()).await,
        Err(Err::NoConnectivity)
    ));
    for (connections, online, urgent) in [
        (json!([wifi(true, false)]), false, false),
        (json!([wifi(true, true)]), true, true),
        (json!([wifi(false, true)]), false, false),
        (
            json!([wifi(false, false), {
                "iface": "Cellular", "name": "Cellular", "primary": true, "has_internet": true
            }]),
            true,
            true,
        ),
        (json!([]), false, false),
    ] {
        update(connections);
        assert_eq!(
            collectors.wait_for_urgent_send().now_or_never().is_some(),
            urgent
        );
        let response = client.req(Default::default()).await;
        if online {
            assert_eq!(response.ok().unwrap().status().as_u16(), 202);
        } else {
            assert!(matches!(response, Err(Err::NoConnectivity)));
        }
    }
    shutdown.cancel();
}
