use fixture::Fixture;
use futures::TryStreamExt;
use orb_connd::{
    network_manager::WifiSec, service::zoci::WifiProfileDto, OrbCapabilities,
};
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use prelude::future::Callback;
use serde_json::json;
use tokio::fs;
use tokio_stream::wrappers::ReadDirStream;
use zenorb::zoci::ReplyExt;

mod fixture;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_increments_priority_when_adding_multiple_networks() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Diamond)
        .release(OrbRelease::Dev)
        .run()
        .await;

    // Act
    let _ = fx
        .zenoh()
        .command(
            "connd/job/wifi_add",
            json!({
                "ssid": "one",
                "sec": "Wpa2Psk",
                "pwd": "qwerty123"
            }),
        )
        .await
        .unwrap();

    let res = fx
        .zenoh()
        .command(
            "connd/job/wifi_add",
            json!({
                "ssid": "two",
                "sec": "Wpa3Sae",
                "pwd": "qwerty124",
                "hidden": true,
            }),
        )
        .await
        .unwrap()
        .unwrap();

    let e = res.payload().try_to_string().unwrap();
    println!("e {e:?}");

    // Assert
    let profiles = fx.nm.list_wifi_profiles().await.unwrap();
    println!("{profiles:?}");

    // profile 0 is default profile
    let profile1 = profiles.get(1).unwrap();
    let profile2 = profiles.get(2).unwrap();

    assert_eq!(profile1.id, "one".to_string());
    assert_eq!(profile1.ssid, "one".to_string());
    assert_eq!(profile1.sec, WifiSec::Wpa2Psk);
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

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_fails_adding_wifi_if_sec_isnt_wpa2psk_or_wpa3sae() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Diamond)
        .release(OrbRelease::Dev)
        .run()
        .await;

    // Act
    let actual1 = fx
        .zenoh()
        .command(
            "connd/job/wifi_add",
            json!({
                "ssid": "one",
                "sec": "owe",
                "pwd": "qwerty123"
            }),
        )
        .await
        .unwrap()
        .unwrap_err();

    let actual2 = fx
        .zenoh()
        .command(
            "connd/job/wifi_add",
            json!({
                "ssid": "two",
                "sec": "fake_val",
                "pwd": "qwerty124",
            }),
        )
        .await
        .unwrap()
        .unwrap_err();

    let actual1 = actual1.payload().try_to_string().unwrap();
    let actual2 = actual2.payload().try_to_string().unwrap();

    // Assert
    let expected = "\"invalid sec. supported values are Wpa2Psk or Wpa3Sae\"";
    assert_eq!(actual1, expected);
    assert_eq!(actual2, expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_removes_a_wifi_profile() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Diamond)
        .release(OrbRelease::Dev)
        .run()
        .await;

    // Act
    let _ = fx
        .zenoh()
        .command(
            "connd/job/wifi_add",
            json!({
                "ssid": "one",
                "sec": "wpa-psk",
                "pwd": "qwerty123",
            }),
        )
        .await
        .unwrap();

    let _ = fx
        .zenoh()
        .command_raw("connd/job/wifi_remove", "one")
        .await
        .unwrap();

    // Assert
    let profiles = fx
        .zenoh()
        .command_raw("connd/job/wifi_list", "")
        .await
        .unwrap()
        .json::<Vec<WifiProfileDto>, String>()
        .unwrap()
        .unwrap();

    assert_eq!(profiles.len(), 1) // default wifi profile should be present
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
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

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_wipes_dhcp_leases_and_seen_bssids_if_too_big() {
    // on an orb, NetworkManager stores its files under:
    // - /usr/persistent/network-manager/connections
    // - /usr/persistent/network-manager/varlib
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Prod)
        .arrange(Callback::new(async |ctx: fixture::Ctx| {
            let varlib = ctx.usr_persistent.join("network-manager").join("varlib");
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

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_protects_default_wifi_and_cellular_profiles() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Dev)
        .run()
        .await;

    // Act
    let cellular_actual = fx
        .zenoh()
        .command(
            "connd/job/wifi_add",
            json!({
                "ssid": "cellular",
                "sec": "wpa-psk",
                "pwd": "12345678",
            }),
        )
        .await
        .unwrap()
        .unwrap_err();

    let wifi_actual = fx
        .zenoh()
        .command(
            "connd/job/wifi_add",
            json!({
                "ssid": "hotspot",
                "sec": "wpa-psk",
                "pwd": "12345678",
            }),
        )
        .await
        .unwrap()
        .unwrap_err();

    let cellular_actual = cellular_actual.payload().try_to_string().unwrap();
    let wifi_actual = wifi_actual.payload().try_to_string().unwrap();

    // Assert
    let cellular_expected = "\"cellular is not an allowed SSID name\"";
    let wifi_expected = "\"hotspot is not an allowed SSID name\"";

    assert_eq!(cellular_actual, cellular_expected);
    assert_eq!(wifi_actual, wifi_expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_returns_saved_wifi_profiles() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Dev)
        .run()
        .await;

    // Act
    let _ = fx
        .zenoh()
        .command(
            "connd/job/wifi_add",
            json!({
                "ssid": "apple",
                "sec": "wpa-psk",
                "pwd": "12345678",
            }),
        )
        .await
        .unwrap();

    let _ = fx
        .zenoh()
        .command(
            "connd/job/wifi_add",
            json!({
                "ssid": "banana",
                "sec": "sae",
                "pwd": "87654321",
            }),
        )
        .await
        .unwrap();

    let actual = fx
        .zenoh()
        .command_raw("connd/job/wifi_list", "")
        .await
        .unwrap()
        .json::<Vec<WifiProfileDto>, String>()
        .unwrap()
        .unwrap();

    // Assert
    let expected = vec![
        WifiProfileDto {
            ssid: "hotspot".into(),
            sec: "Wpa2Psk".into(),
            is_active: false,
        },
        WifiProfileDto {
            ssid: "apple".into(),
            sec: "Wpa2Psk".into(),
            is_active: false,
        },
        WifiProfileDto {
            ssid: "banana".into(),
            sec: "Wpa3Sae".into(),
            is_active: false,
        },
    ];

    assert_eq!(actual, expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_bumps_priority_of_wifi_profile_on_manual_connection_attempt() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .cap(OrbCapabilities::CellularAndWifi)
        .release(OrbRelease::Dev)
        .run()
        .await;

    // Act: create profiles
    let _ = fx
        .zenoh()
        .command(
            "connd/job/wifi_add",
            json!({
                "ssid": "bla",
                "sec": "wpa2",
                "pwd": "12345678",
            }),
        )
        .await
        .unwrap();

    let _ = fx
        .zenoh()
        .command(
            "connd/job/wifi_add",
            json!({
                "ssid": "bla2",
                "sec": "wpa2",
                "pwd": "12345678",
            }),
        )
        .await
        .unwrap();

    // Assert: newest added profile has higher priority
    let profiles = fx.nm.list_wifi_profiles().await.unwrap();
    let bla = profiles.iter().find(|p| p.ssid == "bla").unwrap();
    let bla2 = profiles.iter().find(|p| p.ssid == "bla2").unwrap();
    assert!(bla.priority < bla2.priority);

    // Act: attempt to connect to bla
    let _ = fx
        .zenoh()
        .command_raw("connd/job/wifi_connect", "bla")
        .await
        .unwrap();

    // Assert: last attempted connection profile has higher priority
    let profiles = fx.nm.list_wifi_profiles().await.unwrap();
    let bla = profiles.iter().find(|p| p.ssid == "bla").unwrap();
    let bla2 = profiles.iter().find(|p| p.ssid == "bla2").unwrap();
    assert!(bla.priority > bla2.priority);

    // Act: attempt to connect again to bla
    let _ = fx
        .zenoh()
        .command_raw("connd/job/wifi_connect", "bla")
        .await
        .unwrap();

    // Assert: priority hasn't changed as highest bla was already highest prio
    let profiles = fx.nm.list_wifi_profiles().await.unwrap();
    let new_bla = profiles.iter().find(|p| p.ssid == "bla").unwrap();
    assert!(bla.priority == new_bla.priority);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn profile_is_persisted_after_bumping_priority() {
    // Arrange
    let mut fx = Fixture::platform(OrbOsPlatform::Pearl)
        .cap(OrbCapabilities::CellularAndWifi)
        .release(OrbRelease::Dev)
        .run()
        .await;

    let connd = fx.connd().await;

    // Act: create profile
    let _ = fx
        .zenoh()
        .command(
            "connd/job/wifi_add",
            json!({
                "ssid": "bla",
                "sec": "wpa2",
                "pwd": "12345678",
            }),
        )
        .await
        .unwrap();

    // Act: create second profile with higher priority
    let _ = fx
        .zenoh()
        .command(
            "connd/job/wifi_add",
            json!({
                "ssid": "bla2",
                "sec": "wpa2",
                "pwd": "12345678",
            }),
        )
        .await
        .unwrap();

    // Act: force connect, should rewrite profile to raise priority
    // will fail due to ssid "bla" not existing
    let _ = fx
        .zenoh()
        .command_raw("connd/job/wifi_connect", "bla")
        .await
        .unwrap();

    // Act: restart connd and environment -- profile should be reloaded
    drop(connd);
    fx.restart().await;

    // Assert: both profiles are still persisted
    let profiles = fx.nm.list_wifi_profiles().await.unwrap();
    assert!(profiles.iter().any(|p| p.ssid == "bla2"));
    assert!(profiles.iter().any(|p| p.ssid == "bla"));
}
