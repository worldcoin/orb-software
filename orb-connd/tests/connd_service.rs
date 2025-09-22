use color_eyre::Result;
use nix::libc;
use orb_connd::{
    network_manager::{NetworkManager, WifiProfile, WifiSec},
    service::ConndService,
};
use orb_connd_dbus::ConndProxy;
use orb_info::orb_os_release::OrbRelease;
use std::{path::PathBuf, time::Duration};
use test_utils::docker::{self, Container};
use tokio::{task::JoinHandle, time};
use zbus::Address;

struct Fixture {
    container: Container,
    connd_server_handle: JoinHandle<Result<()>>,
    pub nm: NetworkManager,
    conn: zbus::Connection,
}

impl Fixture {
    async fn new(release: OrbRelease) -> Self {
        let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let docker_ctx = crate_dir.join("tests").join("docker");
        let dockerfile = crate_dir.join("tests").join("docker").join("Dockerfile");
        let tag = "worldcoin-nm";
        docker::build("worldcoin-nm", dockerfile, docker_ctx).await;

        let uid = unsafe { libc::geteuid() };
        let gid = unsafe { libc::getegid() };

        let container = docker::run(
            tag,
            [
                "--pid=host",
                "--userns=host",
                "-e",
                &format!("TARGET_UID={}", uid),
                "-e",
                &format!("TARGET_GID={}", gid),
            ],
        )
        .await;

        time::sleep(Duration::from_secs(1)).await;

        let dbus_socket = container.tempdir.path().join("socket");
        let dbus_socket = format!("unix:path={}", dbus_socket.display());
        let addr: Address = dbus_socket.parse().unwrap();

        // todo: retry for
        let conn = zbus::ConnectionBuilder::address(addr)
            .unwrap()
            .build()
            .await
            .unwrap();

        let connd_server_handle = ConndService::new(conn.clone(), release).spawn();
        time::sleep(Duration::from_secs(1)).await;

        Self {
            nm: NetworkManager::new(conn.clone()),
            conn,
            connd_server_handle,
            container,
        }
    }

    pub async fn connd(&self) -> ConndProxy<'_> {
        ConndProxy::new(&self.conn).await.unwrap()
    }
}

#[tokio::test]
async fn it_increments_priority_when_adding_multiple_networks() {
    // Arrange
    let fx = Fixture::new(OrbRelease::Dev).await;
    let connd = fx.connd().await;

    // Act
    connd
        .add_wifi_profile(
            "one".to_string(),
            "wpa-psk".to_string(),
            "qwerty123".to_string(),
        )
        .await
        .unwrap();

    connd
        .add_wifi_profile(
            "two".to_string(),
            "sae".to_string(),
            "qwerty124".to_string(),
        )
        .await
        .unwrap();

    // Assert
    let profiles = fx.nm.list_wifi_profiles().await.unwrap();

    let actual0 = profiles.get(0).unwrap();
    let actual1 = profiles.get(1).unwrap();

    assert_eq!(actual0.id, "one".to_string());
    assert_eq!(actual0.ssid, "one".to_string());
    assert_eq!(actual0.sec, WifiSec::Wpa2Psk);
    assert_eq!(actual0.pwd, "qwerty123".to_string());
    assert_eq!(actual0.autoconnect, true);
    assert_eq!(actual0.priority, -999);

    assert_eq!(actual1.id, "two".to_string());
    assert_eq!(actual1.ssid, "two".to_string());
    assert_eq!(actual1.sec, WifiSec::Wpa3Sae);
    assert_eq!(actual1.pwd, "qwerty124".to_string());
    assert_eq!(actual1.autoconnect, true);
    assert_eq!(actual1.priority, -998);
}

#[tokio::test]
async fn it_removes_a_wifi_profile() {
    // Arrange
    let fx = Fixture::new(OrbRelease::Dev).await;
    let connd = fx.connd().await;

    // Act
    connd
        .add_wifi_profile(
            "one".to_string(),
            "wpa-psk".to_string(),
            "qwerty123".to_string(),
        )
        .await
        .unwrap();

    connd.remove_wifi_profile("one".to_string()).await.unwrap();

    // Assert
    let profiles = fx.nm.list_wifi_profiles().await.unwrap();
    assert!(profiles.is_empty())
}

#[tokio::test]
async fn it_verifies_netconfig_qr_code() {
    const STAGE: &str = "NETCONFIG:v1.0;WIFI_ENABLED:true;FALLBACK:false;AIRPLANE:false;WIFI:T:WPA;S:network;P:password;;TS:1758277671;SIG:MEYCIQD/HtYGcxwOdNUppjRaGKjSOTnSTI8zJIJH9iDagsT3tAIhAPPq6qgEMGzm6HkRQYpxp86nfDhvUYFrneS2vul4anPA";
    const PROD: &str = "NETCONFIG:v1.0;WIFI_ENABLED:true;FALLBACK:false;AIRPLANE:false;WIFI:T:WPA;S:network;P:password;;TS:1758277966;SIG:MEUCIQCQv9i/eDMLb16yiyN4eXwDh2EGdiL/ZnqEp3HLUPCbAgIgdTOxsd2ApjJRoNjJl/DkuzChINis8AcOMhDWVZe7lPc=";

    let fx = Fixture::new(OrbRelease::Dev).await;
    let connd = fx.connd().await;

    let result0 = connd.apply_netconfig_qr(PROD.into()).await;
    let result1 = connd.apply_netconfig_qr(STAGE.into()).await;

    println!("0 {result0:?} 1 {result1:?}");
}
