use orb_location_wpa_supplicant::WifiNetwork;

use crate::WifiAccessPoint;

use super::{private, WifiInfoProvider};

#[derive(Debug)]
pub struct WpaSupplicantProvider<'a> {
    pub wifi_networks: &'a [WifiNetwork],
}

impl WifiInfoProvider for WpaSupplicantProvider<'_> {}

impl private::Sealed for WpaSupplicantProvider<'_> {
    type Err = eyre::Report;

    fn populate(
        &self,
        request: &mut crate::GeolocationRequest,
    ) -> Result<(), Self::Err> {
        let wifi_access_points: Vec<WifiAccessPoint> = self
            .wifi_networks
            .iter()
            .map(|network| WifiAccessPoint {
                mac_address: network.bssid.clone(),
                signal_strength: network.signal_level,
                age: None,
                channel: Some(network.frequency),
                signal_to_noise_ratio: None,
            })
            .collect();

        request.wifi_access_points = wifi_access_points;

        Ok(())
    }
}
