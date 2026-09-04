#![cfg(feature = "testing")]
use fixture::Fixture;
use futures::TryStreamExt;
use orb_connd::{
    network_manager::{WifiProfile, WifiSec},
    service::{zoci::WifiProfileDto, ConndService, ProfileStorage},
    OrbCapabilities,
};
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use serde_json::json;
use std::time::Duration;
use tokio::fs;
use tokio_stream::wrappers::ReadDirStream;
use zenorb::zoci::ReplyExt;

mod fixture;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_increments_priority_when_adding_multiple_networks() {
    // Arrange
    let mut fx = Fixture::platform(OrbOsPlatform::Diamond)
        .release(OrbRelease::Dev)
        .build()
        .await;

    let handle = fx.run().await;

    // Act
    let _ = handle
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

    let res = handle
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
    let profiles = handle.nm.list_wifi_profiles().await.unwrap();
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
    let mut fx = Fixture::platform(OrbOsPlatform::Diamond)
        .release(OrbRelease::Dev)
        .build()
        .await;

    let handle = fx.run().await;

    // Act
    let actual1 = handle
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

    let actual2 = handle
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
    let mut fx = Fixture::platform(OrbOsPlatform::Diamond)
        .release(OrbRelease::Dev)
        .build()
        .await;

    let handle = fx.run().await;

    // Act
    let _ = handle
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

    let _ = handle
        .zenoh()
        .command_raw("connd/job/wifi_remove", "one")
        .await
        .unwrap();

    // Assert
    let profiles = handle
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
    let mut fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Prod)
        .build()
        .await;

    let handle = fx.run().await;

    // Assert
    let cellular_profiles = handle.nm.list_cellular_profiles().await.unwrap();
    assert_eq!(cellular_profiles.len(), 1);

    let default_cel_profile = cellular_profiles.into_iter().next().unwrap();
    assert_eq!(default_cel_profile.id, "cellular");
    assert_eq!(default_cel_profile.apn, "em");

    let wifi_profiles = handle.nm.list_wifi_profiles().await.unwrap();
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
    let mut fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Prod)
        .build()
        .await;

    let varlib = fx.usr_persistent.join("network-manager").join("varlib");
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

    let _handle = fx.run().await;

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
async fn it_cleans_allocated_lease_space_without_deleting_saved_profiles() {
    // Arrange
    let mut fx = Fixture::platform(OrbOsPlatform::Diamond)
        .release(OrbRelease::Prod)
        .build()
        .await;

    let profiles: Vec<_> = (0..4)
        .map(|n| WifiProfile {
            id: format!("saved-{n}"),
            uuid: uuid::Uuid::new_v4().to_string(),
            ssid: format!("saved-{n}"),
            sec: WifiSec::Wpa2Psk,
            psk: "1234567890".into(),
            autoconnect: true,
            priority: n,
            hidden: false,
            path: String::new(),
        })
        .collect();
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(&profiles, &mut bytes).unwrap();
    let (secure_storage, secure_storage_cancel_token) = fx.run_secure_storage().await;
    secure_storage
        .put("nmprofiles".into(), bytes)
        .await
        .unwrap();

    let varlib = fx.usr_persistent.join("network-manager").join("varlib");
    fs::create_dir_all(&varlib).await.unwrap();
    // Contents total less than 1 MiB, but their allocated space exceeds the limit.
    for n in 0..160 {
        fs::write(varlib.join(format!("{n}.lease")), [1; 4097])
            .await
            .unwrap();
    }
    fs::write(varlib.join("secret_key"), "preserve-identity")
        .await
        .unwrap();

    // Act
    let handle = fx
        .run_with()
        .secure_storage(secure_storage)
        .secure_storage_cancel_token(secure_storage_cancel_token)
        .call()
        .await;

    // Assert
    let nm_profiles = handle.nm.list_wifi_profiles().await.unwrap();
    assert_eq!(nm_profiles.len(), profiles.len() + 1);
    for profile in &profiles {
        assert!(nm_profiles.iter().any(|p| p.ssid == profile.ssid));
    }
    let bytes = handle
        .secure_storage
        .get("nmprofiles".into())
        .await
        .unwrap()
        .unwrap();
    let stored: Vec<WifiProfile> = ciborium::de::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(stored.len(), profiles.len() + 1);
    for profile in &profiles {
        assert!(stored.iter().any(|p| p.ssid == profile.ssid));
    }

    for n in 0..160 {
        assert!(!fs::try_exists(varlib.join(format!("{n}.lease")))
            .await
            .unwrap());
    }
    assert_eq!(
        fs::read_to_string(varlib.join("secret_key")).await.unwrap(),
        "preserve-identity"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_cleans_leases_without_evicting_profiles_when_secure_storage_fails() {
    // Arrange
    let mut fx = Fixture::platform(OrbOsPlatform::Diamond)
        .release(OrbRelease::Prod)
        .build()
        .await;
    let handle = fx.run().await;
    for n in 0..4 {
        handle
            .zenoh()
            .command(
                "connd/job/wifi_add",
                json!({
                    "ssid": format!("saved-{n}"),
                    "sec": "Wpa2Psk",
                    "pwd": "1234567890"
                }),
            )
            .await
            .unwrap()
            .unwrap();
    }

    let connd = ConndService::new(
        handle.dbus.clone(),
        handle.nm.clone(),
        OrbRelease::Prod,
        OrbCapabilities::WifiOnly,
        Duration::from_secs(1),
        &fx.usr_persistent,
        ProfileStorage::SecureStorage(handle.secure_storage.clone()),
    )
    .await
    .unwrap();
    let mut before: Vec<_> = handle
        .nm
        .list_wifi_profiles()
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.uuid)
        .collect();
    before.sort();
    assert_eq!(before.len(), 5);

    handle.secure_storage_cancel_token.cancel();
    tokio::time::timeout(Duration::from_secs(5), async {
        while handle.secure_storage.get("nmprofiles".into()).await.is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("stopped secure storage must return an error");

    let varlib = fx.usr_persistent.join("network-manager").join("varlib");
    fs::create_dir_all(&varlib).await.unwrap();
    let lease = varlib.join("old.lease");
    fs::write(&lease, vec![1; 2 * 1024 * 1024]).await.unwrap();
    // Keep local state above the limit after lease deletion to exercise the
    // guard against profile eviction when secure storage cannot be measured.
    let other_state = varlib.join("NetworkManager.state");
    fs::write(&other_state, vec![1; 2 * 1024 * 1024])
        .await
        .unwrap();

    // Act
    let error = tokio::time::timeout(
        Duration::from_secs(5),
        connd.ensure_nm_state_below_max_size(&fx.usr_persistent),
    )
    .await
    .expect("local cleanup must finish despite the secure-storage failure")
    .unwrap_err();

    // Assert
    assert!(
        format!("{error:?}").contains("failed to read nmprofiles from secure storage")
    );
    assert!(!fs::try_exists(&lease).await.unwrap());
    assert_eq!(
        fs::read(&other_state).await.unwrap(),
        vec![1; 2 * 1024 * 1024]
    );
    let mut after: Vec<_> = handle
        .nm
        .list_wifi_profiles()
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.uuid)
        .collect();
    after.sort();
    assert_eq!(before, after);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_protects_default_wifi_and_cellular_profiles() {
    // Arrange
    let mut fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Dev)
        .build()
        .await;

    let handle = fx.run().await;

    // Act
    let cellular_actual = handle
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

    let wifi_actual = handle
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
    let mut fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Dev)
        .build()
        .await;

    let handle = fx.run().await;

    // Act
    let _ = handle
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

    let _ = handle
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

    let actual = handle
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
    let mut fx = Fixture::platform(OrbOsPlatform::Pearl)
        .cap(OrbCapabilities::CellularAndWifi)
        .release(OrbRelease::Dev)
        .build()
        .await;

    let handle = fx.run().await;

    // Act: create profiles
    let _ = handle
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

    let _ = handle
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
    let profiles = handle.nm.list_wifi_profiles().await.unwrap();
    let bla = profiles.iter().find(|p| p.ssid == "bla").unwrap();
    let bla2 = profiles.iter().find(|p| p.ssid == "bla2").unwrap();
    assert!(bla.priority < bla2.priority);

    // Act: attempt to connect to bla
    let _ = handle
        .zenoh()
        .command_raw("connd/job/wifi_connect", "bla")
        .await
        .unwrap();

    // Assert: last attempted connection profile has higher priority
    let profiles = handle.nm.list_wifi_profiles().await.unwrap();
    let bla = profiles.iter().find(|p| p.ssid == "bla").unwrap();
    let bla2 = profiles.iter().find(|p| p.ssid == "bla2").unwrap();
    assert!(bla.priority > bla2.priority);

    // Act: attempt to connect again to bla
    let _ = handle
        .zenoh()
        .command_raw("connd/job/wifi_connect", "bla")
        .await
        .unwrap();

    // Assert: priority hasn't changed as highest bla was already highest prio
    let profiles = handle.nm.list_wifi_profiles().await.unwrap();
    let new_bla = profiles.iter().find(|p| p.ssid == "bla").unwrap();
    assert!(bla.priority == new_bla.priority);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn profile_is_persisted_after_bumping_priority() {
    // Arrange
    let mut fx = Fixture::platform(OrbOsPlatform::Pearl)
        .cap(OrbCapabilities::CellularAndWifi)
        .release(OrbRelease::Dev)
        .build()
        .await;

    let handle = fx.run().await;
    let connd = handle.connd().await;

    // Act: create profile
    let _ = handle
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
    let _ = handle
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
    let _ = handle
        .zenoh()
        .command_raw("connd/job/wifi_connect", "bla")
        .await
        .unwrap();

    // Act: restart connd and environment -- profile should be reloaded
    drop(connd);
    handle.stop().await;
    let handle = fx.run().await;

    // Assert: both profiles are still persisted
    let profiles = handle.nm.list_wifi_profiles().await.unwrap();
    assert!(profiles.iter().any(|p| p.ssid == "bla2"));
    assert!(profiles.iter().any(|p| p.ssid == "bla"));
}
