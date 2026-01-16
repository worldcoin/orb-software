use crate::fixture::Fixture;
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use rkyv::rancor;
use std::time::Duration;
use tokio::time;

mod fixture;

#[tokio::test(flavor = "multi_thread")]
async fn it_publishes_net_changed() {
    // Arrange
    println!("starting!!!!");
    let fx = Fixture::platform(OrbOsPlatform::Diamond)
        .release(OrbRelease::Dev)
        .log(true)
        .run()
        .await;

    let zenoh = fx.zenoh().await;
    println!(
        "SUBSCRIBING!! {}",
        format!("dev/{}/connd/net/changed", fx.orb_id)
    );

    let sub = zenoh
        .declare_subscriber(format!("dev/{}/connd/net/changed", fx.orb_id))
        .await
        .unwrap();

    // Act
    let msg = time::timeout(Duration::from_secs(2), sub.recv_async())
        .await
        .unwrap()
        .unwrap();

    let bytes = msg.payload().to_bytes();
    let archived =
        rkyv::access::<orb_connd_events::ArchivedConnection, rancor::Error>(&bytes[..])
            .unwrap();

    // Assert
    match archived {
        orb_connd_events::ArchivedConnection::ConnectedGlobal(_) => (),
        _ => panic!("should be connected, got {archived:?}"),
    }
}
