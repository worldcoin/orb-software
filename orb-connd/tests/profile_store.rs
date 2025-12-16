use std::time::Duration;

use fixture::Fixture;
use orb_connd::{
    network_manager::{WifiProfile, WifiSec},
    profile_store::ProfileStore,
    OrbCapabilities,
};
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use prelude::future::Callback;
use tokio::time;
use uuid::Uuid;

mod fixture;

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn it_imports_persisted_nm_profiles_and_deletes_them_on_startup() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .cap(OrbCapabilities::WifiOnly)
        .release(OrbRelease::Prod)
        .arrange(Callback::new(async |ctx: fixture::Ctx| {
            // prepopulate with persisted nm profiles
            ctx.nm
                .wifi_profile("imported")
                .ssid("imported")
                .sec(WifiSec::Wpa2Psk)
                .psk("1234567890")
                .persist(true)
                .add()
                .await
                .unwrap();
        }))
        .run()
        .await;

    let connd = fx.connd().await;

    time::sleep(Duration::from_secs(5)).await;

    // Assert profile is still in nm
    let profiles = connd.list_wifi_profiles().await.unwrap();
    let imported_profile = profiles.iter().find(|p| p.ssid == "imported");

    assert!(imported_profile.is_some(), "{profiles:?}");

    // Assert profile is in store
    let profile_store = ProfileStore::new(fx.secure_storage.clone());
    profile_store.import().await.unwrap();

    let ssids: Vec<_> = profile_store.values().into_iter().map(|p| p.ssid).collect();
    assert_eq!(vec!["imported"], ssids);

    // Assert profile is no longer on disk
    let out = fx
        .container
        .exec(&["ls", "/etc/NetworkManager/system-connections/"])
        .await
        .stdout;

    let out = String::from_utf8_lossy(&out);
    assert!(out.trim().is_empty(), "{out}");
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn it_adds_removes_and_imports_encrypted_profiles_on_startup() {
    // Arrage
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .cap(OrbCapabilities::WifiOnly)
        .release(OrbRelease::Prod)
        .arrange(Callback::new(async |ctx: fixture::Ctx| {
            // prepopulate with encrypted profiles
            let profile_store = ProfileStore::new(ctx.secure_storage);

            profile_store.insert(WifiProfile {
                id: "imported".into(),
                uuid: Uuid::new_v4().to_string(),
                ssid: "imported".into(),
                sec: WifiSec::Wpa2Psk,
                psk: "1234567890".into(),
                autoconnect: false,
                priority: 0,
                hidden: false,
                path: String::new(),
            });

            profile_store.commit().await.unwrap();
        }))
        .run()
        .await;

    let connd = fx.connd().await;

    // Assert profile was imported
    let imported_profile = connd
        .list_wifi_profiles()
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.ssid == "imported");

    assert!(imported_profile.is_some());

    // Act: remove imported, add new profile
    connd.remove_wifi_profile("imported".into()).await.unwrap();
    connd
        .add_wifi_profile(
            "new_profile".into(),
            "Wpa2Psk".into(),
            "1234567890".into(),
            false,
        )
        .await
        .unwrap();

    // Assert: store reflects changes
    let profile_store = ProfileStore::new(fx.secure_storage.clone());
    profile_store.import().await.unwrap();

    let ssids: Vec<_> = profile_store.values().into_iter().map(|p| p.ssid).collect();
    assert_eq!(vec!["new_profile"], ssids);
}
