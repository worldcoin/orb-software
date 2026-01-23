use fixture::Fixture;
use orb_connd::OrbCapabilities;
use orb_connd_dbus::ConnectionState;
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use std::time::Duration;
use tokio::time;

mod fixture;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_does_not_change_netconfig_if_no_cellular() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .cap(OrbCapabilities::WifiOnly)
        .release(OrbRelease::Dev)
        .run()
        .await;

    let connd = fx.connd().await;

    // Act
    let actual = connd
        .netconfig_set(true, false, false)
        .await
        .unwrap_err()
        .to_string();

    // Assert
    let expected =
        "org.freedesktop.DBus.Error.Failed: cannot apply netconfig on orbs that do not have cellular";

    assert_eq!(actual, expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_sets_and_gets_netconfig() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .cap(OrbCapabilities::CellularAndWifi)
        .release(OrbRelease::Dev)
        .run()
        .await;

    let connd = fx.connd().await;

    // Act
    let res = connd.netconfig_set(false, false, false).await.unwrap();
    let netcfg = connd.netconfig_get().await.unwrap();

    // Assert
    assert_eq!(res, netcfg);
    assert!(!netcfg.wifi);
    assert!(!netcfg.smart_switching);
    assert!(!netcfg.airplane_mode);

    // Act
    let res = connd.netconfig_set(true, true, false).await.unwrap();
    let netcfg = connd.netconfig_get().await.unwrap();

    // Assert
    assert_eq!(res, netcfg);
    assert!(netcfg.wifi);
    assert!(netcfg.smart_switching);
    assert!(!netcfg.airplane_mode);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_returns_connected_connection_state() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .cap(OrbCapabilities::CellularAndWifi)
        .release(OrbRelease::Dev)
        .run()
        .await;

    let connd = fx.connd().await;

    // Act
    let state = connd.connection_state().await.unwrap();

    // Assert
    assert_eq!(state, ConnectionState::Connected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_returns_partial_connection_state() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .cap(OrbCapabilities::CellularAndWifi)
        .release(OrbRelease::Dev)
        .run()
        .await;

    // change connectivity check uri
    let out = fx.container
        .exec(&[
            "sed",
            "-i",
            "-E",
            r#"/^\[connectivity\]/,/^\[/{s|^[[:space:]]*uri=.*$|uri=http://fakeuri.com|}"#,
            "/etc/NetworkManager/NetworkManager.conf",
        ])
        .await;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stdout: {stdout}\nstderr: {stderr}");

    // reload network manager to apply new connectivity check uri
    let out = fx.container.exec(&["nmcli", "general", "reload"]).await;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stdout: {stdout}\nstderr: {stderr}");

    // wait enough time for nmcli to reload
    time::sleep(Duration::from_secs(1)).await;

    let connd = fx.connd().await;

    // Act
    let state = connd.connection_state().await.unwrap();

    // Assert
    assert_eq!(state, ConnectionState::PartiallyConnected);
}
