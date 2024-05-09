const IFACE: &str = "wlan0";

#[tokio::main]
async fn main() {
    let status = orb_wpa_supplicant::iface_status(IFACE)
        .await
        .expect("failed to get status");
    println!("status: {status:?}");
    let ssid = orb_wpa_supplicant::current_network_ssid(IFACE)
        .await
        .expect("failed to get ssid");
    println!("ssid: {ssid:?}");
    let rssi = orb_wpa_supplicant::current_network_rssi(IFACE)
        .await
        .expect("failed to get rssi");
    println!("rssi: {rssi:?}");
}
