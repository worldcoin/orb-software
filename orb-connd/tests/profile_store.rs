#![cfg(feature = "testing")]
use fixture::Fixture;
use orb_connd::{
    network_manager::{WifiProfile, WifiSec},
    service::zoci::WifiProfileDto,
    OrbCapabilities,
};
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use serde_json::json;
use uuid::Uuid;
use zenorb::zoci::ReplyExt;

mod fixture;

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_adds_removes_and_imports_encrypted_profiles() {
    // Arrange
    let mut fx = Fixture::platform(OrbOsPlatform::Diamond)
        .cap(OrbCapabilities::WifiOnly)
        .release(OrbRelease::Prod)
        .build()
        .await;

    // prepopulate with encrypted profiles
    let profiles = vec![WifiProfile {
        id: "imported".into(),
        uuid: Uuid::new_v4().to_string(),
        ssid: "imported".into(),
        sec: WifiSec::Wpa2Psk,
        psk: "1234567890".into(),
        autoconnect: false,
        priority: 0,
        hidden: false,
        path: String::new(),
    }];

    let mut bytes = Vec::new();
    ciborium::ser::into_writer(&profiles, &mut bytes).unwrap();

    let (secure_storage, secure_storage_cancel_token) = fx.run_secure_storage().await;

    secure_storage
        .put("nmprofiles".into(), bytes)
        .await
        .unwrap();

    let handle = fx
        .run_with()
        .secure_storage(secure_storage)
        .secure_storage_cancel_token(secure_storage_cancel_token)
        .call()
        .await;

    // Assert profile was imported
    let imported_profile = handle
        .zenoh()
        .command_raw("connd/job/wifi_list", "")
        .await
        .unwrap()
        .json::<Vec<WifiProfileDto>, String>()
        .unwrap()
        .unwrap()
        .into_iter()
        .find(|p| p.ssid == "imported");

    assert!(imported_profile.is_some());

    // Act: remove imported, add new profile
    let _ = handle
        .zenoh()
        .command_raw("connd/job/wifi_remove", "imported")
        .await
        .unwrap();

    let _ = handle
        .zenoh()
        .command(
            "connd/job/wifi_add",
            json!({
                "ssid": "new_profile",
                "sec": "Wpa2Psk",
                "pwd": "1234567890"
            }),
        )
        .await
        .unwrap();

    // Assert: store reflects changes
    let profiles = handle
        .secure_storage
        .get("nmprofiles".into())
        .await
        .unwrap()
        .unwrap();

    let profiles: Vec<WifiProfile> =
        ciborium::de::from_reader(profiles.as_slice()).unwrap();

    let ssids: Vec<_> = profiles.into_iter().map(|p| p.ssid).collect();
    assert_eq!(vec!["hotspot", "new_profile"], ssids);
}
