use async_tempfile::TempDir;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use zenorb::{zenoh, Zenorb};

const ROUTER_CONFIG: &str = include_str!("../../android/zenohd/zenohd.json5");

fn router_config(endpoint: &str) -> zenoh::Config {
    let mut config = zenoh::Config::from_json5(ROUTER_CONFIG).unwrap();
    config
        .insert_json5(
            "listen/endpoints",
            &serde_json::to_string(&[endpoint]).unwrap(),
        )
        .unwrap();
    config
}

#[test]
fn android_router_config_uses_only_local_uds() {
    let config = zenoh::Config::from_json5(ROUTER_CONFIG).unwrap();
    for (key, expected) in [
        ("mode", r#""router""#),
        (
            "listen/endpoints",
            r#"["unixsock-stream//data/local/tmp/zenohd.sock"]"#,
        ),
        ("connect/endpoints", "[]"),
        ("scouting/multicast/enabled", "false"),
        ("scouting/gossip/enabled", "false"),
        ("plugins", "{}"),
    ] {
        assert_eq!(config.get_json(key).unwrap(), expected, "{key}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn routes_events_between_clients_over_uds() {
    timeout(Duration::from_secs(15), async {
        let dir = TempDir::new().await.unwrap();
        let endpoint = format!(
            "unixsock-stream/{}/router.sock",
            dir.to_path_buf().display()
        );
        let router = zenoh::open(router_config(&endpoint)).await.unwrap();
        let mut config = zenorb::default_cfg();
        config
            .insert_json5(
                "connect/endpoints",
                &serde_json::to_string(&[&endpoint]).unwrap(),
            )
            .unwrap();
        let publisher = Zenorb::from_cfg(config.clone())
            .orb_id("00000000".parse().unwrap())
            .retries(0)
            .with_name("orb-engine")
            .await
            .unwrap();
        let consumer = Zenorb::from_cfg(config)
            .orb_id("00000000".parse().unwrap())
            .retries(0)
            .with_name("orb-backend-status")
            .await
            .unwrap();
        let subscriber = consumer.declare_subscriber("**/oes/**").await.unwrap();
        let sender = publisher
            .sender()
            .publisher("oes/test")
            .build()
            .await
            .unwrap();
        let event_publisher = sender.publisher("oes/test").unwrap();
        while !event_publisher.matching_status().await.unwrap().matching() {
            sleep(Duration::from_millis(10)).await;
        }
        event_publisher.put(r#"{"value":42}"#).await.unwrap();
        let sample = subscriber.recv_async().await.unwrap();
        assert_eq!(sample.key_expr().as_str(), "00000000/orb-engine/oes/test");
        assert_eq!(sample.payload().try_to_string().unwrap(), r#"{"value":42}"#);
        router.close().await.unwrap();
    })
    .await
    .expect("UDS routing timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn router_reports_unwritable_socket_location() {
    let dir = TempDir::new().await.unwrap();
    let parent = dir.to_path_buf().join("not-a-directory");
    tokio::fs::write(&parent, "").await.unwrap();
    let endpoint = format!("unixsock-stream/{}/router.sock", parent.display());
    let result = timeout(
        Duration::from_secs(5),
        zenoh::open(router_config(&endpoint)),
    )
    .await
    .expect("router startup timed out");
    assert!(
        result.is_err(),
        "router must fail if its socket cannot be created"
    );
}
