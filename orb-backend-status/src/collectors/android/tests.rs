use super::*;
use crate::{
    backend::client::{Err, StatusClient},
    orb_event_stream::{Event, Payload},
    sender::BackendSender,
};
use orb_dogd::test::MetricSinkhole;
use std::time::Duration;
use tokio::time::timeout;
use wiremock::{
    matchers::{basic_auth, method},
    Mock, MockServer, ResponseTemplate,
};

async fn setup(token: &str, server: &MockServer) -> (Collectors, StatusClient) {
    let (collectors, token_rx, connectivity_rx) = Collectors::new(
        Config {
            token: token.to_owned(),
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();

    let client = StatusClient::builder()
        .metrics(MetricSinkhole)
        .orb_id("ea2ea744".parse().unwrap())
        .orb_name("android-test".parse().unwrap())
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
            token: "static-token".to_owned(),
        },
        shutdown.clone(),
    )
    .await
    .unwrap();

    assert_eq!(&*token_rx.borrow(), "static-token");
    assert!(connectivity_rx.borrow().is_connected());
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
async fn sends_cached_oes_with_initial_static_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(basic_auth("ea2ea744", "static-token"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let (collectors, client) = setup("static-token", &server).await;
    let shutdown = CancellationToken::new();
    let oes = OrbEventStream::start(client.clone(), shutdown.clone());
    oes.ingest(Payload {
        headers: oes::Headers::default().mode(oes::Mode::CacheOnly),
        event: Event {
            name: "orb-engine/test".to_owned(),
            created_at: 123,
            payload: Some(serde_json::json!({ "value": 42 })),
        },
    })
    .unwrap();

    let sender = BackendSender::new(client, oes, Duration::from_secs(30));
    assert!(timeout(
        Duration::from_secs(5),
        sender.send_snapshot(collectors.snapshot().await)
    )
    .await
    .unwrap()
    .unwrap());
    shutdown.cancel();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = requests[0].body_json().unwrap();
    assert_eq!(body["orb_id"], "ea2ea744");
    assert_eq!(body["oes_cached"], true);
    assert_eq!(body["oes"][0]["name"], "orb-engine/test");
    assert_eq!(body["oes"][0]["payload"]["value"], 42);
    assert!(body["update_progress"].is_null());
    assert!(body["wifi"].is_null());
}

#[tokio::test]
async fn empty_static_token_keeps_existing_missing_token_error() {
    let server = MockServer::start().await;
    let (collectors, client) = setup("", &server).await;
    let result = timeout(
        Duration::from_secs(5),
        client.req(collectors.snapshot().await),
    )
    .await
    .unwrap();

    assert!(matches!(result, Err(Err::MissingAttestToken)));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn backend_failures_do_not_count_as_successful_snapshots() {
    for response in [
        ResponseTemplate::new(503),
        ResponseTemplate::new(200).set_delay(Duration::from_secs(1)),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(response)
            .expect(1..=4)
            .mount(&server)
            .await;
        let (collectors, client) = setup("static-token", &server).await;
        let shutdown = CancellationToken::new();
        let oes = OrbEventStream::start(client.clone(), shutdown.clone());
        let sender = BackendSender::new(client, oes, Duration::from_secs(30));

        assert!(timeout(
            Duration::from_secs(5),
            sender.send_snapshot(collectors.snapshot().await)
        )
        .await
        .unwrap()
        .is_err());
        shutdown.cancel();
    }
}
