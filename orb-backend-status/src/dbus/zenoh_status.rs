use crate::{
    collectors::hardware_states::HardwareState, dbus::intf_impl::BackendStatusImpl,
};
use orb_messages::main::AmbientLight;
use std::collections::HashMap;

/// Status collected from zenoh: hardware states and front ALS (ambient
/// light sensor) readings.
#[derive(Debug, Default, Clone)]
pub struct ZenohStatus {
    pub hardware_states: Option<HashMap<String, HardwareState>>,
    pub front_als: Option<AmbientLight>,
}

impl BackendStatusImpl {
    /// Update hardware states from zenoh.
    pub fn update_hardware_states(&self, states: HashMap<String, HardwareState>) {
        let Ok(mut current_status) = self.current_status.lock() else {
            return;
        };
        current_status.zenoh.hardware_states = if states.is_empty() {
            None
        } else {
            Some(states)
        };
    }

    /// Update front ALS (Ambient Light Sensor) data from zenoh.
    pub fn update_front_als(&self, als: Option<AmbientLight>) {
        let Ok(mut current_status) = self.current_status.lock() else {
            return;
        };
        current_status.zenoh.front_als = als;
    }

    /// Update the active SSID in the current status.
    /// Called by the connectivity watcher when SSID changes via zenoh.
    /// If connd_report doesn't exist yet, creates a minimal one. A no-op
    /// without the `dbus` feature: `connd_report` doesn't exist then.
    pub fn update_active_ssid(&self, ssid: Option<String>) {
        #[cfg(feature = "dbus")]
        {
            use orb_backend_status_dbus::types::ConndReport;

            let Ok(mut current_status) = self.current_status.lock() else {
                return;
            };

            match &mut current_status.dbus.connd_report {
                Some(report) => {
                    report.active_wifi_profile = ssid;
                }
                None => {
                    // Create minimal connd_report with just the SSID
                    current_status.dbus.connd_report = Some(ConndReport {
                        egress_iface: None,
                        wifi_enabled: true,
                        smart_switching: false,
                        airplane_mode: false,
                        active_wifi_profile: ssid,
                        saved_wifi_profiles: vec![],
                        scanned_networks: vec![],
                    });
                }
            }
        }

        #[cfg(not(feature = "dbus"))]
        let _ = ssid;
    }
}
