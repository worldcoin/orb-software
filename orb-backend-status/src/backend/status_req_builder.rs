use super::types::OrbStatusApiV2;
#[cfg(feature = "dbus")]
use super::types::{
    ConndReportApiV2, LocationDataApiV2, NetIntfApiV2, NetStatsApiV2,
    UpdateProgressApiV2, WifiProfileApiV2,
};
#[cfg(feature = "zenoh")]
use crate::backend::types::{AmbientLightApiV2, HardwareStateApiV2, MainMcuApiV2};
#[cfg(feature = "dbus")]
use crate::backend::types::{
    BatteryApiV2, CellularStatusApiV2, SsdStatusApiV2, TemperatureApiV2, WifiApiV2,
    WifiDataApiV2, WifiQualityApiV2,
};
use crate::{backend::uptime::orb_uptime, dbus::intf_impl::CurrentStatus};
use chrono::Utc;
use tracing::warn;

impl CurrentStatus {
    pub async fn to_orb_status_api_v2_req(&self) -> OrbStatusApiV2 {
        let uptime_sec = orb_uptime()
            .await
            .inspect_err(|e| warn!("failed to read orb uptime: {e:?}"))
            .ok();

        let mut req = OrbStatusApiV2 {
            uptime_sec,
            timestamp: Utc::now(),
            ..Default::default()
        };

        #[cfg(feature = "dbus")]
        self.apply_dbus_fields(&mut req);
        #[cfg(feature = "zenoh")]
        self.apply_zenoh_fields(&mut req);

        req
    }

    /// Populates the legacy D-Bus-sourced fields (update-agent progress,
    /// orb-core signup/stats, connd's WiFi/cellular report).
    #[cfg(feature = "dbus")]
    fn apply_dbus_fields(&self, req: &mut OrbStatusApiV2) {
        req.location_data =
            self.dbus
                .wifi_networks
                .as_ref()
                .map(|wifi_networks| LocationDataApiV2 {
                    wifi: Some(
                        wifi_networks
                            .iter()
                            .map(|w| WifiDataApiV2 {
                                ssid: Some(w.ssid.clone()),
                                bssid: Some(w.bssid.clone()),
                                signal_strength: Some(w.signal_level),
                                frequency: Some(w.frequency),
                                channel: freq_to_channel(w.frequency),
                                signal_to_noise_ratio: None,
                            })
                            .collect(),
                    ),
                    gps: None,
                    cell: None,
                });

        req.update_progress =
            self.dbus.update_progress.as_ref().map(|update_progress| {
                UpdateProgressApiV2 {
                    download_progress: update_progress.download_progress,
                    processed_progress: update_progress.processed_progress,
                    install_progress: update_progress.install_progress,
                    total_progress: update_progress.total_progress,
                    error: update_progress.error.clone(),
                    state: update_progress.state,
                }
            });

        req.net_stats = self.dbus.net_stats.as_ref().map(|net_stats| NetStatsApiV2 {
            interfaces: net_stats
                .interfaces
                .iter()
                .map(|i| NetIntfApiV2 {
                    name: i.name.clone(),
                    tx_bytes: i.tx_bytes,
                    rx_bytes: i.rx_bytes,
                    tx_packets: i.tx_packets,
                    rx_packets: i.rx_packets,
                    tx_errors: i.tx_errors,
                    rx_errors: i.rx_errors,
                })
                .collect(),
        });

        req.battery = self
            .dbus
            .core_stats
            .as_ref()
            .map(|core_stats| BatteryApiV2 {
                level: Some(core_stats.battery.level),
                is_charging: Some(core_stats.battery.is_charging),
            });

        req.mac_address = self
            .dbus
            .core_stats
            .as_ref()
            .map(|core_stats| core_stats.mac_address.clone());

        req.ssd = self
            .dbus
            .core_stats
            .as_ref()
            .map(|core_stats| SsdStatusApiV2 {
                file_left: Some(core_stats.ssd.file_left),
                space_left: Some(core_stats.ssd.space_left),
                signup_left_to_upload: Some(core_stats.ssd.signup_left_to_upload),
            });

        req.temperature =
            self.dbus
                .core_stats
                .as_ref()
                .map(|core_stats| TemperatureApiV2 {
                    cpu: Some(core_stats.temperature.cpu),
                    gpu: Some(core_stats.temperature.gpu),
                    front_unit: Some(core_stats.temperature.front_unit),
                    front_pcb: Some(core_stats.temperature.front_pcb),
                    battery_pcb: Some(core_stats.temperature.battery_pcb),
                    battery_cell: Some(core_stats.temperature.battery_cell),
                    backup_battery: Some(core_stats.temperature.backup_battery),
                    liquid_lens: Some(core_stats.temperature.liquid_lens),
                    main_accelerometer: Some(core_stats.temperature.main_accelerometer),
                    main_mcu: Some(core_stats.temperature.main_mcu),
                    mainboard: Some(core_stats.temperature.mainboard),
                    security_accelerometer: Some(
                        core_stats.temperature.security_accelerometer,
                    ),
                    security_mcu: Some(core_stats.temperature.security_mcu),
                    battery_pack: Some(core_stats.temperature.battery_pack),
                    ssd: Some(core_stats.temperature.ssd),
                });

        req.wifi = self
            .dbus
            .connd_report
            .as_ref()
            .and_then(|connd_report| {
                connd_report.scanned_networks.iter().find(|n| {
                    connd_report
                        .active_wifi_profile
                        .as_ref()
                        .is_some_and(|p| p == &n.ssid)
                })
            })
            .map(|wifi| WifiApiV2 {
                ssid: Some(wifi.ssid.clone()),
                bssid: Some(wifi.bssid.clone()),
                frequency: Some(wifi.frequency),
                quality: Some(WifiQualityApiV2 {
                    signal_level: Some(wifi.signal_level),
                    bit_rate: None,
                    link_quality: None,
                    noise_level: None,
                }),
            });

        req.signup_state = self
            .dbus
            .signup_state
            .as_ref()
            .map(|state| state.to_string());

        req.cellular_status = self
            .dbus
            .cellular_status
            .as_ref()
            // backend requires ICCID to be Some otherwise it will fail deserialization
            // of CellularStatusApiV2. So if ICCID is None, the struct itself should be None.
            .and_then(|cs| cs.iccid.as_ref().map(|iccid| (cs, iccid)))
            .map(|(cs, iccid)| CellularStatusApiV2 {
                imei: cs.imei.clone(),
                fw_revision: cs.fw_revision.clone(),
                iccid: iccid.to_owned(),
                rat: cs.rat.clone(),
                operator: cs.operator.clone(),
                rsrp: cs.rsrp,
                rsrq: cs.rsrq,
                rssi: cs.rssi,
                snr: cs.snr,
            });

        req.connd_report = self.dbus.connd_report.as_ref().map(|r| ConndReportApiV2 {
            egress_iface: r.egress_iface.clone(),
            wifi_enabled: r.wifi_enabled,
            smart_switching: r.smart_switching,
            airplane_mode: r.airplane_mode,
            active_wifi_profile: r.active_wifi_profile.clone(),
            saved_wifi_profiles: r
                .saved_wifi_profiles
                .iter()
                .map(|p| WifiProfileApiV2 {
                    ssid: p.ssid.clone(),
                    sec: p.sec.clone(),
                })
                .collect(),
        });

        req.orb_stand_qr_id = self
            .dbus
            .core_stats
            .as_ref()
            .and_then(|core_stats| core_stats.orb_stand_qr_id.clone());

        req.orb_stand_qr_type = self
            .dbus
            .core_stats
            .as_ref()
            .and_then(|core_stats| core_stats.orb_stand_qr_type.clone());
    }

    /// Populates the zenoh-sourced fields (hardware states, front ALS).
    #[cfg(feature = "zenoh")]
    fn apply_zenoh_fields(&self, req: &mut OrbStatusApiV2) {
        req.hardware_states = self.zenoh.hardware_states.as_ref().map(|states| {
            states
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        HardwareStateApiV2 {
                            status: v.status.clone(),
                            message: v.message.clone(),
                        },
                    )
                })
                .collect()
        });

        let front_als = self.zenoh.front_als.as_ref().map(|als| AmbientLightApiV2 {
            ambient_light_lux: als.ambient_light_lux,
            flag: crate::collectors::front_als::flag_to_api_str(als.flag).to_string(),
        });

        // Only set main_mcu if there's at least one field populated
        req.main_mcu = front_als.is_some().then_some(MainMcuApiV2 { front_als });
    }
}

// Helper function to convert frequency to channel number
#[cfg(feature = "dbus")]
fn freq_to_channel(freq: u32) -> Option<u32> {
    // For 2.4 GHz: channel = (freq - 2412) / 5 + 1
    if (2412..=2484).contains(&freq) {
        if freq == 2484 {
            // Special case for channel 14
            return Some(14);
        }
        return Some((freq - 2412) / 5 + 1);
    }

    // For 5 GHz: varies by region, but generally channel = (freq - 5000) / 5
    if (5170..=5825).contains(&freq) {
        return Some((freq - 5000) / 5);
    }

    // For 6 GHz (Wi-Fi 6E): channel = (freq - 5950) / 5 + 1
    if (5955..=7115).contains(&freq) {
        return Some((freq - 5950) / 5 + 1);
    }

    None
}

#[cfg(all(test, feature = "dbus"))]
mod tests {
    use super::*;
    use crate::dbus::intf_impl::DbusStatus;
    use orb_backend_status_dbus::types::{SignupState, WifiNetwork};

    #[tokio::test]
    async fn test_build_status_request_v2() {
        let wifi_networks = vec![WifiNetwork {
            bssid: "00:11:22:33:44:55".into(),
            frequency: 2412,
            signal_level: -45,
            ssid: "TestAP".into(),
        }];
        let request = CurrentStatus {
            dbus: DbusStatus {
                wifi_networks: Some(wifi_networks),
                signup_state: Some(SignupState::Ready),
                ..Default::default()
            },
            ..Default::default()
        }
        .to_orb_status_api_v2_req()
        .await;

        assert!(request.timestamp <= Utc::now());
        let location_data = request
            .location_data
            .expect("Location data should be present");

        assert!(
            location_data.cell.is_none(),
            "Cell data should not be present"
        );

        let wifi_data = location_data.wifi.expect("WiFi data should be present");
        assert_eq!(wifi_data.len(), 1);
        let wifi = &wifi_data[0];

        assert_eq!(wifi.bssid, Some("00:11:22:33:44:55".to_string()));
        assert_eq!(wifi.ssid, Some("TestAP".to_string()));
        assert_eq!(wifi.signal_strength, Some(-45));
        assert_eq!(wifi.channel, Some(1)); // 2412 MHz = channel 1
        assert_eq!(wifi.signal_to_noise_ratio, None);

        let signup_state = request
            .signup_state
            .expect("Signup state should be present");
        assert_eq!(signup_state, "Ready");
    }

    #[tokio::test]
    async fn test_freq_to_channel_conversion() {
        // 2.4 GHz band
        assert_eq!(freq_to_channel(2412), Some(1));
        assert_eq!(freq_to_channel(2437), Some(6));
        assert_eq!(freq_to_channel(2472), Some(13));
        assert_eq!(freq_to_channel(2484), Some(14));

        // 5 GHz band
        assert_eq!(freq_to_channel(5180), Some(36));
        assert_eq!(freq_to_channel(5500), Some(100));

        // 6 GHz band
        assert_eq!(freq_to_channel(5955), Some(2));
        assert_eq!(freq_to_channel(6175), Some(46));

        // Invalid frequencies
        assert_eq!(freq_to_channel(1000), None);
        assert_eq!(freq_to_channel(9000), None);
    }
}
