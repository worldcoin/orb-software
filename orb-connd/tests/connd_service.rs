use fixture::Fixture;
use futures::{future, TryStreamExt};
use orb_connd::{network_manager::WifiSec, OrbCapabilities};
use orb_connd_dbus::WifiProfile;
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use prelude::future::Callback;
use std::path::PathBuf;
use tokio::fs;
use tokio_stream::wrappers::ReadDirStream;

mod fixture;

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn it_increments_priority_when_adding_multiple_networks() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Diamond)
        .release(OrbRelease::Dev)
        .run()
        .await;

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

    // profile 0 is default profile
    let profile1 = profiles.get(1).unwrap();
    let profile2 = profiles.get(2).unwrap();

    assert_eq!(profile1.id, "one".to_string());
    assert_eq!(profile1.ssid, "one".to_string());
    assert_eq!(profile1.sec, WifiSec::WpaPsk);
    assert_eq!(profile1.psk, "qwerty123".to_string());
    assert!(profile1.autoconnect);
    assert_eq!(profile1.priority, -997);
    assert!(!profile1.hidden);

    assert_eq!(profile2.id, "two".to_string());
    assert_eq!(profile2.ssid, "two".to_string());
    assert_eq!(profile2.sec, WifiSec::Wpa3Sae);
    assert_eq!(profile2.psk, "qwerty124".to_string());
    assert!(profile2.autoconnect);
    assert_eq!(profile2.priority, -996);
    assert!(profile2.hidden);
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn it_removes_a_wifi_profile() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Diamond)
        .release(OrbRelease::Dev)
        .run()
        .await;

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
    assert_eq!(profiles.len(), 1) // default wifi profile should be present
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
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
        let fx = Fixture::platform(OrbOsPlatform::Diamond)
            .release(release)
            .run()
            .await;

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
        assert_eq!(profile.psk, "password");
        assert!(!profile.hidden);
        assert!(!fx.nm.smart_switching_enabled().await.unwrap());
        assert!(fx.nm.wifi_enabled().await.unwrap());
    });

    future::join_all(tests).await;
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn it_does_not_apply_netconfig_if_ts_is_too_old() {
    // todo
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn it_applies_wifi_qr_code() {
    // Arrange (dev orbs)
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Dev)
        .run()
        .await;

    let connd = fx.connd().await;

    // Act
    connd
        .apply_wifi_qr("WIFI:S:example;T:WPA;P:1234567890;H:true;;".into())
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

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn it_creates_default_profiles() {
    // Arrange & Act
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Prod)
        .run()
        .await;

    // Assert
    let cellular_profiles = fx.nm.list_cellular_profiles().await.unwrap();
    assert_eq!(cellular_profiles.len(), 1);

    let default_cel_profile = cellular_profiles.into_iter().next().unwrap();
    assert_eq!(default_cel_profile.id, "cellular");
    assert_eq!(default_cel_profile.apn, "em");

    let wifi_profiles = fx.nm.list_wifi_profiles().await.unwrap();
    assert_eq!(wifi_profiles.len(), 1);

    let default_wifi_profile = wifi_profiles.into_iter().next().unwrap();
    assert_eq!(default_wifi_profile.ssid, "hotspot");
    assert!(default_wifi_profile.autoconnect);
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
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
        .await;

    assert!(result.is_err());

    // Act
    connd.apply_magic_reset_qr().await.unwrap();

    // Assert: all wifi profiles except default deleted
    let profiles = fx.nm.list_wifi_profiles().await.unwrap();
    assert_eq!(profiles.len(), 1); // len is 1 bc default wifi profile was created

    // Assert: applying a new wifi qr code now succeeds even if we have connectivity
    let result = connd
        .apply_wifi_qr("WIFI:S:example;T:WPA;P:1234567890;H:true;;".into())
        .await;

    assert!(result.is_ok());
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn it_wipes_dhcp_leases_and_seen_bssids_if_too_big() {
    // on an orb, NetworkManager stores its files under:
    // - /usr/persistent/network-manager/connections
    // - /usr/persistent/network-manager/varlib
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Prod)
        .arrange(Callback::new(async |usr_persistent: PathBuf| {
            let varlib = usr_persistent.join("network-manager").join("varlib");
            fs::create_dir_all(&varlib).await.unwrap();

            // we create a file thats 2mb in size, which puts us
            // above the 1mb limit for network-manager folder in /usr/persistent
            let contents = vec![0u8; 2 * 1024 * 1024];
            fs::write(varlib.join("seen-bssids"), &contents)
                .await
                .unwrap();

            for n in 0..30 {
                fs::write(varlib.join(format!("{n}.lease")), [])
                    .await
                    .unwrap();
            }

            let dir: Vec<_> = ReadDirStream::new(fs::read_dir(varlib).await.unwrap())
                .try_collect()
                .await
                .unwrap();

            assert_eq!(31, dir.len());
        }))
        .run()
        .await;

    // Assert
    // after connd starts, it should check if nm folder in persistent is over limit,
    // and if so deletes seen-bssids file and all .lease files.
    let varlib = fx.usr_persistent.join("network-manager").join("varlib");
    let dir: Vec<_> = ReadDirStream::new(fs::read_dir(varlib).await.unwrap())
        .try_collect()
        .await
        .unwrap();

    for d in &dir {
        println!("{:?}", d.file_name());
    }

    assert!(dir.is_empty())
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn it_protects_default_wifi_and_cellular_profiles() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Dev)
        .run()
        .await;

    let connd = fx.connd().await;

    // Act
    let cellular_actual = connd
        .add_wifi_profile(
            "cellular".into(),
            "wpa-psk".into(),
            "12345678".into(),
            false,
        )
        .await
        .unwrap_err()
        .to_string();

    let wifi_actual = connd
        .add_wifi_profile("hotspot".into(), "wpa-psk".into(), "12345678".into(), false)
        .await
        .unwrap_err()
        .to_string();

    // Assert
    let cellular_expected =
        "org.freedesktop.DBus.Error.Failed: cellular is not an allowed SSID name";
    let wifi_expected =
        "org.freedesktop.DBus.Error.Failed: hotspot is not an allowed SSID name";

    assert_eq!(cellular_actual, cellular_expected);
    assert_eq!(wifi_actual, wifi_expected);
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn it_returns_saved_wifi_profiles() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Dev)
        .run()
        .await;

    let connd = fx.connd().await;

    // Act
    connd
        .add_wifi_profile("apple".into(), "wpa-psk".into(), "12345678".into(), false)
        .await
        .unwrap();
    connd
        .add_wifi_profile("banana".into(), "sae".into(), "87654321".into(), false)
        .await
        .unwrap();

    let actual = connd.list_wifi_profiles().await.unwrap();

    // Assert
    let expected = vec![
        WifiProfile {
            ssid: "hotspot".into(),
            sec: "wpa2".into(),
            psk: "easytotypehardtoguess".into(),
        },
        WifiProfile {
            ssid: "apple".into(),
            sec: "wpa2".into(),
            psk: "12345678".into(),
        },
        WifiProfile {
            ssid: "banana".into(),
            sec: "wpa3".into(),
            psk: "87654321".into(),
        },
    ];

    assert_eq!(actual, expected);
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
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

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
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
