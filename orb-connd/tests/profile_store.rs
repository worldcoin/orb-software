use fixture::Fixture;
use orb_connd::{
    network_manager::{WifiProfile, WifiSec},
    OrbCapabilities,
};
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use prelude::future::Callback;
use uuid::Uuid;

mod fixture;

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn it_adds_removes_and_imports_encrypted_profiles() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Diamond)
        .cap(OrbCapabilities::WifiOnly)
        .release(OrbRelease::Prod)
        .arrange(Callback::new(async |ctx: fixture::Ctx| {
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

            ctx.secure_storage
                .put("nmprofiles".into(), bytes)
                .await
                .unwrap();
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
    let profiles = fx.secure_storage.get("nmprofiles".into()).await.unwrap();
    let profiles: Vec<WifiProfile> =
        ciborium::de::from_reader(profiles.as_slice()).unwrap();

    let ssids: Vec<_> = profiles.into_iter().map(|p| p.ssid).collect();
    assert_eq!(vec!["hotspot", "new_profile"], ssids);
}
