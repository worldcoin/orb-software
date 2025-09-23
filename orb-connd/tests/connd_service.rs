use color_eyre::Result;
use futures::future;
use nix::libc;
use orb_connd::{
    network_manager::{NetworkManager, WifiSec},
    service::ConndService,
};
use orb_connd_dbus::ConndProxy;
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use std::{path::PathBuf, time::Duration};
use test_utils::docker::{self, Container};
use tokio::{task::JoinHandle, time};
use zbus::Address;

#[allow(dead_code)]
struct Fixture {
    pub nm: NetworkManager,
    container: Container,
    connd_server_handle: JoinHandle<Result<()>>,
    conn: zbus::Connection,
}

impl Fixture {
    async fn new(release: OrbRelease, platform: OrbOsPlatform) -> Self {
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

        let connd_server_handle =
            ConndService::new(conn.clone(), conn.clone(), release, platform).spawn();

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
    let fx = Fixture::new(OrbRelease::Dev, OrbOsPlatform::Diamond).await;
    let connd = fx.connd().await;

    // Act
    connd
        .add_wifi_profile(
            "one".to_string(),
            "wpa-psk".to_string(),
            "qwerty123".to_string(),
            false,
        )
        .await
        .unwrap();

    connd
        .add_wifi_profile(
            "two".to_string(),
            "sae".to_string(),
            "qwerty124".to_string(),
            true,
        )
        .await
        .unwrap();

    // Assert
    let profiles = fx.nm.list_wifi_profiles().await.unwrap();

    // 0 is the default wifi profile
    let actual0 = profiles.get(1).unwrap();
    let actual1 = profiles.get(2).unwrap();

    assert_eq!(actual0.id, "one".to_string());
    assert_eq!(actual0.ssid, "one".to_string());
    assert_eq!(actual0.sec, WifiSec::WpaPsk);
    assert_eq!(actual0.pwd, "qwerty123".to_string());
    assert_eq!(actual0.autoconnect, true);
    assert_eq!(actual0.priority, -998);
    assert_eq!(actual0.hidden, false);

    assert_eq!(actual1.id, "two".to_string());
    assert_eq!(actual1.ssid, "two".to_string());
    assert_eq!(actual1.sec, WifiSec::Wpa3Sae);
    assert_eq!(actual1.pwd, "qwerty124".to_string());
    assert_eq!(actual1.autoconnect, true);
    assert_eq!(actual1.priority, -997);
    assert_eq!(actual1.hidden, true);
}

#[tokio::test]
async fn it_removes_a_wifi_profile() {
    // Arrange
    let fx = Fixture::new(OrbRelease::Dev, OrbOsPlatform::Diamond).await;
    let connd = fx.connd().await;

    // Act
    connd
        .add_wifi_profile(
            "one".to_string(),
            "wpa-psk".to_string(),
            "qwerty123".to_string(),
            false,
        )
        .await
        .unwrap();

    connd.remove_wifi_profile("one".to_string()).await.unwrap();

    // Assert
    let profiles = fx.nm.list_wifi_profiles().await.unwrap();
    assert!(profiles.len() == 1) // default profile should still be here
}

#[tokio::test]
async fn it_applies_netconfig_qr_code() {
    // Arrange
    const STAGE: &str = "NETCONFIG:v1.0;WIFI_ENABLED:true;FALLBACK:false;AIRPLANE:false;WIFI:T:WPA;S:network;P:password;;TS:1758277671;SIG:MEYCIQD/HtYGcxwOdNUppjRaGKjSOTnSTI8zJIJH9iDagsT3tAIhAPPq6qgEMGzm6HkRQYpxp86nfDhvUYFrneS2vul4anPA";
    const PROD: &str = "NETCONFIG:v1.0;WIFI_ENABLED:true;FALLBACK:false;AIRPLANE:false;WIFI:T:WPA;S:network;P:password;;TS:1758277966;SIG:MEUCIQCQv9i/eDMLb16yiyN4eXwDh2EGdiL/ZnqEp3HLUPCbAgIgdTOxsd2ApjJRoNjJl/DkuzChINis8AcOMhDWVZe7lPc=";

    let tests = [
        (OrbRelease::Dev, STAGE, true),
        (OrbRelease::Dev, PROD, false),
        (OrbRelease::Service, STAGE, false),
        (OrbRelease::Service, PROD, true),
        (OrbRelease::Analysis, STAGE, false),
        (OrbRelease::Analysis, PROD, true),
        (OrbRelease::Prod, STAGE, false),
        (OrbRelease::Prod, PROD, true),
    ]
    .into_iter()
    .map(async |(release, netconfig, is_ok)| {
        let fx = Fixture::new(release, OrbOsPlatform::Diamond).await;
        let connd = fx.connd().await;

        // Act
        let result = connd.apply_netconfig_qr(netconfig.into(), false).await;

        // Assert
        assert_eq!(
            result.is_ok(),
            is_ok,
            "{release}, {netconfig}, is_ok: {is_ok}"
        );

        if !is_ok {
            return;
        }

        let profile = fx
            .nm
            .list_wifi_profiles()
            .await
            .unwrap()
            .into_iter()
            .find(|profile| profile.id == "network")
            .unwrap();

        assert_eq!(profile.ssid, "network");
        assert_eq!(profile.pwd, "password");
        assert_eq!(profile.hidden, false);
        assert!(!fx.nm.smart_switching_enabled().await.unwrap());
        assert!(fx.nm.wifi_enabled().await.unwrap());
    });

    future::join_all(tests).await;
}

#[tokio::test]
async fn it_does_not_apply_netconfig_if_ts_is_too_old() {
    // todo
}

#[tokio::test]
async fn it_applies_wifi_qr_code() {
    // Arrange (dev orbs)
    let fx = Fixture::new(OrbRelease::Dev, OrbOsPlatform::Diamond).await;
    let connd = fx.connd().await;

    // Act
    connd
        .apply_wifi_qr("WIFI:S:example;T:WPA;P:1234567890;H:true;".into())
        .await
        .unwrap();

    // Assert
    let profile = fx
        .nm
        .list_wifi_profiles()
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.id == "example")
        .unwrap();

    assert_eq!(profile.ssid, "example");
    assert_eq!(profile.sec, WifiSec::WpaPsk);
    assert_eq!(profile.pwd, "1234567890");
    assert_eq!(profile.autoconnect, true);
    assert_eq!(profile.hidden, true);

    // Arrange (prod orbs, fails if there is connectivity, which we do bc this is in a container and host has connectivity)
    let fx = Fixture::new(OrbRelease::Prod, OrbOsPlatform::Diamond).await;
    let connd = fx.connd().await;

    // Act
    let result = connd
        .apply_wifi_qr("WIFI:S:example;T:WPA;P:1234567890;H:true;".into())
        .await;

    // Assert
    assert!(result.is_err());
}

#[tokio::test]
async fn it_creates_default_profiles() {
    // todo
}

#[tokio::test]
async fn it_applies_magic_reset_qr() {
    // Arrange
    let fx = Fixture::new(OrbRelease::Prod, OrbOsPlatform::Diamond).await;
    let connd = fx.connd().await;

    // profile added, should be erased once magic qr is applied
    connd
        .add_wifi_profile("ssid".into(), "wpa".into(), "qwery12345678".into(), false)
        .await
        .unwrap();

    connd
        .add_wifi_profile("ssid2".into(), "wpa".into(), "qwery12345678".into(), false)
        .await
        .unwrap();

    // should fail -- we have internet connectivity
    let result = connd
        .apply_wifi_qr("WIFI:S:example;T:WPA;P:1234567890;H:true;".into())
        .await;

    assert!(result.is_err());

    // Act
    connd.apply_magic_reset_qr().await.unwrap();

    // Assert: all wifi profiles except default deleted
    let profiles = fx.nm.list_wifi_profiles().await.unwrap();
    assert_eq!(profiles.len(), 1);

    // Assert: applying a new wifi qr code now succeeds even if we have connectivity
    let result = connd
        .apply_wifi_qr("WIFI:S:example;T:WPA;P:1234567890;H:true;".into())
        .await;

    assert!(result.is_ok());
}
