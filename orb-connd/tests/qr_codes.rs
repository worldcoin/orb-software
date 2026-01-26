use fixture::Fixture;
use futures::future;
use orb_connd::{network_manager::WifiSec, OrbCapabilities};
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};

mod fixture;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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
        let fx = Fixture::platform(OrbOsPlatform::Diamond)
            .cap(OrbCapabilities::CellularAndWifi)
            .release(release)
            .run()
            .await;

        let connd = fx.connd().await;

        // Act
        // we unwrap the error here because attempting to connect will NOT work
        // as NM is running in a container.
        // but we assert error the one from the very last step of attempting to connect
        let result = connd
            .apply_netconfig_qr(netconfig.into(), false)
            .await
            .unwrap_err()
            .to_string();

        // Assert
        if !is_ok {
            assert_eq!(result, "org.freedesktop.DBus.Error.Failed: verification of qr sig failed: signature error");
            return;
        }

        assert_eq!(result, "org.freedesktop.DBus.Error.Failed: could not find ssid network");

        let profile = fx
            .nm
            .list_wifi_profiles()
            .await
            .unwrap()
            .into_iter()
            .find(|profile| profile.id == "network")
            .unwrap();

        assert_eq!(profile.ssid, "network");
        assert_eq!(profile.psk, "password");
        assert!(!profile.hidden);
        assert!(!fx.nm.smart_switching_enabled().await.unwrap());
        assert!(fx.nm.wifi_enabled().await.unwrap());
    });

    future::join_all(tests).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_applies_wifi_qr_code() {
    // Arrange (dev orbs)
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Dev)
        .run()
        .await;

    let connd = fx.connd().await;

    // Act
    // we unwrap the error here because attempting to connect will NOT work
    // as NM is running in a container.
    // but we assert error the one from the very last step of attempting to connect
    let result = connd
        .apply_wifi_qr("WIFI:S:example;T:WPA;P:1234567890;H:true;;".into())
        .await
        .unwrap_err()
        .to_string();

    // Assert
    assert_eq!(
        result,
        "org.freedesktop.DBus.Error.Failed: could not find ssid example"
    );

    let profile = fx
        .nm
        .list_wifi_profiles()
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.id == "example")
        .unwrap();

    assert_eq!(profile.ssid, "example");
    assert_eq!(profile.sec, WifiSec::Wpa2Psk);
    assert_eq!(profile.psk, "1234567890");
    assert!(profile.autoconnect);
    assert!(profile.hidden);

    // Arrange (prod orbs, fails if there is connectivity, which we do bc this is in a container and host has connectivity)
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Prod)
        .run()
        .await;

    let connd = fx.connd().await;

    // Act
    let result = connd
        .apply_wifi_qr("WIFI:S:example;T:WPA;P:1234567890;H:true;".into())
        .await;

    // Assert
    assert!(result.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_applies_magic_reset_qr() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Prod)
        .run()
        .await;

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
        .apply_wifi_qr("WIFI:S:example;T:WPA;P:1234567890;H:true;;".into())
        .await
        .unwrap_err()
        .to_string();

    assert_eq!(
        result,
        "org.freedesktop.DBus.Error.Failed: we already have internet connectivity, use signed qr instead"
    );

    // Act
    connd.apply_magic_reset_qr().await.unwrap();

    // Assert: all wifi profiles except default deleted
    let profiles = fx.nm.list_wifi_profiles().await.unwrap();
    assert_eq!(profiles.len(), 1); // len is 1 bc default wifi profile was created

    // Assert: applying a new wifi qr code now succeeds even if we have connectivity
    // we unwrap the error here because attempting to connect will NOT work
    // as NM is running in a container.
    // but we assert error the one from the very last step of attempting to connect
    let result = connd
        .apply_wifi_qr("WIFI:S:example;T:WPA;P:1234567890;H:true;;".into())
        .await
        .unwrap_err()
        .to_string();

    assert_eq!(
        result,
        "org.freedesktop.DBus.Error.Failed: could not find ssid example"
    );
}
