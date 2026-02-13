use fixture::Fixture;
use orb_connd::network_manager::WifiSec;
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use prelude::future::Callback;
use tokio::fs;

mod fixture;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_imports_wpa_conf_with_hex_encoded_ssid() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Dev)
        .arrange(Callback::new(async |ctx: fixture::Ctx| {
            // Create wpa_supplicant config with hex-encoded SSID
            // SSID "546573744e6574776f726b" is hex for "TestNetwork"
            let wpa_conf_content = r#"ctrl_interface=DIR=/var/run/wpa_supplicant GROUP=netdev

network={
    key_mgmt=WPA-PSK
    ssid=546573744e6574776f726b
    psk=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
}
"#;
            fs::write(
                ctx.usr_persistent.join("wpa_supplicant-wlan0.conf"),
                wpa_conf_content,
            )
            .await
            .unwrap();
        }))
        .run()
        .await;

    // Assert - the hex-encoded SSID should be decoded to "TestNetwork"
    let profiles = fx.nm.list_wifi_profiles().await.unwrap();
    let test_profile = profiles
        .iter()
        .find(|p| p.ssid == "TestNetwork")
        .expect("TestNetwork profile should exist");

    assert_eq!(test_profile.ssid, "TestNetwork");
    assert_eq!(
        test_profile.psk,
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    );
    assert_eq!(test_profile.sec, WifiSec::Wpa2Psk);

    // Assert - wpa_supplicant config file should be deleted
    let config_path = fx.usr_persistent.join("wpa_supplicant-wlan0.conf");
    assert!(!fs::try_exists(&config_path).await.unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_imports_wpa_conf_with_quoted_ssid() {
    // Arrange
    let fx = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Dev)
        .arrange(Callback::new(async |ctx: fixture::Ctx| {
            // Create wpa_supplicant config with quoted SSID
            let wpa_conf_content = r#"ctrl_interface=DIR=/var/run/wpa_supplicant GROUP=netdev

network={
    key_mgmt=WPA-PSK
    ssid="MyNetwork"
    psk=fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210
}
"#;
            fs::write(
                ctx.usr_persistent.join("wpa_supplicant-wlan0.conf"),
                wpa_conf_content,
            )
            .await
            .unwrap();
        }))
        .run()
        .await;

    // Assert - the quoted SSID should be parsed correctly
    let profiles = fx.nm.list_wifi_profiles().await.unwrap();
    let network_profile = profiles
        .iter()
        .find(|p| p.ssid == "MyNetwork")
        .expect("MyNetwork profile should exist");

    assert_eq!(network_profile.ssid, "MyNetwork");
    assert_eq!(
        network_profile.psk,
        "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
    );
    assert_eq!(network_profile.sec, WifiSec::Wpa2Psk);

    // Assert - wpa_supplicant config file should be deleted
    let config_path = fx.usr_persistent.join("wpa_supplicant-wlan0.conf");
    assert!(!fs::try_exists(&config_path).await.unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_handles_invalid_wpa_conf_gracefully() {
    // Test empty SSID (quoted)
    {
        let fx = Fixture::platform(OrbOsPlatform::Pearl)
            .release(OrbRelease::Dev)
            .arrange(Callback::new(async |ctx: fixture::Ctx| {
                let wpa_conf_content = r#"ctrl_interface=DIR=/var/run/wpa_supplicant GROUP=netdev
network={
    key_mgmt=WPA-PSK
    ssid=""
    psk=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
}
"#;
                fs::write(
                    ctx.usr_persistent.join("wpa_supplicant-wlan0.conf"),
                    wpa_conf_content,
                )
                .await
                .unwrap();
            }))
            .run()
            .await;

        // Should fail gracefully - no crash, just empty result
        let profiles = fx.nm.list_wifi_profiles().await.unwrap();
        // Only default profile should exist (import should have failed gracefully)
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].ssid, "hotspot");
    }

    // Test empty PSK
    {
        let fx = Fixture::platform(OrbOsPlatform::Pearl)
            .release(OrbRelease::Dev)
            .arrange(Callback::new(async |ctx: fixture::Ctx| {
                let wpa_conf_content = r#"ctrl_interface=DIR=/var/run/wpa_supplicant GROUP=netdev
network={
    key_mgmt=WPA-PSK
    ssid="ValidSSID"
    psk=
}
"#;
                fs::write(
                    ctx.usr_persistent.join("wpa_supplicant-wlan0.conf"),
                    wpa_conf_content,
                )
                .await
                .unwrap();
            }))
            .run()
            .await;

        let profiles = fx.nm.list_wifi_profiles().await.unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].ssid, "hotspot");
    }

    // Test SSID too long (>32 bytes)
    {
        let fx = Fixture::platform(OrbOsPlatform::Pearl)
            .release(OrbRelease::Dev)
            .arrange(Callback::new(async |ctx: fixture::Ctx| {
                let long_ssid = "a".repeat(33);
                let wpa_conf_content = format!(
                    r#"ctrl_interface=DIR=/var/run/wpa_supplicant GROUP=netdev
network={{
    key_mgmt=WPA-PSK
    ssid="{long_ssid}"
    psk=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
}}
"#
                );
                fs::write(
                    ctx.usr_persistent.join("wpa_supplicant-wlan0.conf"),
                    wpa_conf_content,
                )
                .await
                .unwrap();
            }))
            .run()
            .await;

        let profiles = fx.nm.list_wifi_profiles().await.unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].ssid, "hotspot");
    }
}
