use crate::fixture::Fixture;
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use std::time::Duration;
use tokio::time;

mod fixture;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_publishes_net_changed() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Diamond)
        .release(OrbRelease::Dev)
        .run()
        .await;

    let zenoh = fx.zenoh().await;

    // Act
    time::sleep(Duration::from_secs(2)).await;

    let get = zenoh
        .get(format!("{}/connd/oes/active_connections", fx.orb_id))
        .await
        .unwrap();

    let msg = time::timeout(Duration::from_secs(2), get.recv_async())
        .await
        .unwrap()
        .unwrap()
        .into_result()
        .unwrap();

    let active_conns: oes::ActiveConnections =
        serde_json::from_slice(&msg.payload().to_bytes()).unwrap();

    // Assert
    // this is Disconnected, because there is no primary connection (we are using host internet
    // and not a connection from network manager), and the event depends on having a primary connection
    let expected = oes::ActiveConnections {
        connectivity_uri: "http://connectivity-check.worldcoin.org".into(),
        connections: vec![],
    };

    assert_eq!(active_conns, expected);
}
