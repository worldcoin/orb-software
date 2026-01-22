use crate::fixture::Fixture;
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use rkyv::AlignedVec;
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
        .get(format!("{}/connd/net/changed", fx.orb_id))
        .await
        .unwrap();

    let msg = time::timeout(Duration::from_secs(2), get.recv_async())
        .await
        .unwrap()
        .unwrap()
        .into_result()
        .unwrap();

    let mut bytes = AlignedVec::with_capacity(msg.payload().len());
    bytes.extend_from_slice(&msg.payload().to_bytes());
    let archived =
        rkyv::check_archived_root::<orb_connd_events::Connection>(&bytes).unwrap();

    // Assert
    // this is Disconnected, because there is no primary connection (we are using host internet
    // and not a connection from network manager), and the event depends on having a primary connection
    assert_eq!(
        archived,
        &orb_connd_events::ArchivedConnection::Disconnected
    );
}
