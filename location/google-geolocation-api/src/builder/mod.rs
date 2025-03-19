//! This module provides the ability to build a [`GeolocationRequest`] in a generic
//! way using a [type-state builder pattern].
//!
//! It is generic to enable *multiple* implementations of different sources of wifi
//! or cellular info to co-exist without baking those dependencies into this crate.
//!
//! Concrete implementations for different info providers can be found gated as feature flags.
//!
//! [type-state]: https://zerotomastery.io/blog/rust-typestate-patterns/

#[cfg(feature = "cellcom")]
mod cellcom;
#[cfg(feature = "wpa-supp")]
mod wpa_supp;

use std::marker::PhantomData;

use crate::GeolocationRequest;

#[cfg(feature = "cellcom")]
pub use self::cellcom::CellcomProvider;

#[cfg(feature = "wpa-supp")]
pub use self::wpa_supp::WpaSupplicantProvider;

mod private {
    use super::*;

    /// Private trait that prevents third parties from implementing this - we want all
    /// implementations to live in `orb-google-geolocation-api`.
    ///
    /// For an explanation of what sealed traits are, read
    /// [here](https://predr.ag/blog/definitive-guide-to-sealed-traits-in-rust/#what-are-sealed-traits)
    pub trait Sealed {
        type Err;
        fn populate(&self, request: &mut GeolocationRequest) -> Result<(), Self::Err>;
    }
}

/// Generic way to build requests from cell info. Implement this for your particular
/// hardware peripheral.
pub trait CellInfoProvider: private::Sealed {}

/// Generic way to build requests from cell info. Implement this for your particular
/// wifi subsystem (wpa_supplicant, network manager, etc).
pub trait WifiInfoProvider: private::Sealed {}

#[derive(Debug)]
pub struct GeolocationRequestBuilder<S = WantsWifi> {
    request: GeolocationRequest,
    _phantom: PhantomData<S>,
}

impl<S> Default for GeolocationRequestBuilder<S> {
    fn default() -> Self {
        Self {
            request: GeolocationRequest::default(),
            _phantom: Default::default(),
        }
    }
}

impl<S1> GeolocationRequestBuilder<S1> {
    fn to<S2>(self) -> GeolocationRequestBuilder<S2> {
        GeolocationRequestBuilder::<S2> {
            request: self.request,
            _phantom: Default::default(),
        }
    }
}

/// The builder requires wifi data to be provided.
#[derive(Debug)]
pub struct WantsWifi;

impl GeolocationRequestBuilder<WantsWifi> {
    pub fn wifi<W>(
        mut self,
        wifi: &W,
    ) -> Result<GeolocationRequestBuilder<WantsCell>, W::Err>
    where
        W: WifiInfoProvider,
    {
        wifi.populate(&mut self.request)?;
        Ok(self.to())
    }
}

/// The builder requires cellular data to be provided.
#[derive(Debug)]
pub struct WantsCell;

impl GeolocationRequestBuilder<WantsCell> {
    pub fn cell<C>(
        mut self,
        cell: &C,
    ) -> Result<GeolocationRequestBuilder<Done>, C::Err>
    where
        C: CellInfoProvider,
    {
        cell.populate(&mut self.request)?;

        Ok(self.to())
    }
}

/// The builder is done.
#[derive(Debug)]
pub struct Done;

impl GeolocationRequestBuilder<Done> {
    pub fn finish(self) -> GeolocationRequest {
        self.request
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cellcom::CellcomProvider;
    use orb_cellcom::data::ServingCell;
    use orb_location_wpa_supplicant::WifiNetwork;
    use wpa_supp::WpaSupplicantProvider;

    #[test]
    fn test_build_geolocation_request_valid() {
        let serving_cell = ServingCell {
            connection_status: "CONNECT".to_string(),
            network_type: "LTE".to_string(),
            duplex_mode: "FDD".to_string(),
            mcc: Some(310),
            mnc: Some(260),
            cell_id: "00AB12".to_string(), // hex
            channel_or_arfcn: Some(100),
            pcid_or_psc: Some(22),
            rsrp: Some(-90),
            rsrq: Some(-10),
            rssi: Some(-60),
            sinr: Some(12),
        };

        let wifi_networks = &[WifiNetwork {
            bssid: "00:11:22:33:44:55".into(),
            frequency: 2412,
            signal_level: -45,
            flags: "[WPA2-PSK-CCMP][ESS]".into(),
            ssid: "TestAP".into(),
        }];

        let req = GeolocationRequest::builder()
            .wifi(&WpaSupplicantProvider { wifi_networks })
            .unwrap()
            .cell(&CellcomProvider {
                serving_cell: &serving_cell,
            })
            .unwrap()
            .finish();

        assert_eq!(req.cell_towers.len(), 1);
        assert_eq!(req.wifi_access_points.len(), 1);

        let tower = &req.cell_towers[0];

        // 0x00AB12 => 70130 decimal
        assert_eq!(tower.cell_id, 0x00AB12);
        assert_eq!(tower.mobile_country_code, Some(310));
        assert_eq!(tower.mobile_network_code, Some(260));

        let ap = &req.wifi_access_points[0];
        assert_eq!(ap.mac_address, "00:11:22:33:44:55");
        assert_eq!(ap.signal_strength, -45);
        assert_eq!(ap.channel, Some(2412));
    }

    #[test]
    fn test_build_geolocation_request_invalid_hex() {
        let serving_cell = ServingCell {
            connection_status: "CONNECT".to_string(),
            network_type: "LTE".to_string(),
            duplex_mode: "FDD".to_string(),
            mcc: Some(310),
            mnc: Some(260),
            cell_id: "GARBAGE".to_string(),
            channel_or_arfcn: None,
            pcid_or_psc: None,
            rsrp: None,
            rsrq: None,
            rssi: None,
            sinr: None,
        };

        let wifi_networks = &[];
        let err = GeolocationRequest::builder()
            .wifi(&WpaSupplicantProvider { wifi_networks })
            .unwrap()
            .cell(&CellcomProvider {
                serving_cell: &serving_cell,
            })
            .unwrap_err();

        assert!(err.to_string().contains("invalid digit"));
    }
}
