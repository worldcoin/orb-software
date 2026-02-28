use crate::collectors::hardware_states::HardwareState;
use orb_backend_status_dbus::{
    types::{
        CellularStatus, ConndReport, CoreStats, NetStats, SignupState, UpdateProgress,
        WifiNetwork,
    },
    BackendStatusT,
};
use orb_messages::main::AmbientLight;
use orb_update_agent_dbus::UpdateAgentState;

use orb_telemetry::TraceCtx;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;
use tracing::{error, info_span};

#[derive(Clone)]
pub struct BackendStatusImpl {
    current_status: Arc<Mutex<CurrentStatus>>,
    /// Notify to wake up the sender loop immediately for urgent sends.
    notify_urgent: Arc<Notify>,
    /// Flag that persists until we actually send. Set by urgent events,
    /// cleared only after successful send.
    send_immediately: Arc<Mutex<bool>>,
}

#[derive(Debug, Default, Clone)]
pub struct CurrentStatus {
    pub wifi_networks: Option<Vec<WifiNetwork>>,
    pub update_progress: Option<UpdateProgress>,
    pub net_stats: Option<NetStats>,
    pub cellular_status: Option<CellularStatus>,
    pub core_stats: Option<CoreStats>,
    pub signup_state: Option<SignupState>,
    pub connd_report: Option<ConndReport>,
    pub hardware_states: Option<HashMap<String, HardwareState>>,
    pub front_als: Option<AmbientLight>,
}

impl BackendStatusT for BackendStatusImpl {
    fn provide_update_progress(
        &self,
        update_progress: UpdateProgress,
        trace_ctx: TraceCtx,
    ) -> zbus::fdo::Result<()> {
        let span = info_span!("backend-status::provide_update_progress");
        trace_ctx.apply(&span);
        let _guard = span.enter();

        let Ok(mut current_status) = self
            .current_status
            .lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
        else {
            return Ok(());
        };

        if update_progress.state == UpdateAgentState::Rebooting {
            self.set_send_immediately();
        }

        current_status.update_progress = Some(update_progress);

        Ok(())
    }

    fn provide_net_stats(
        &self,
        net_stats: NetStats,
        trace_ctx: TraceCtx,
    ) -> zbus::fdo::Result<()> {
        let span = info_span!("backend-status::provide_net_stats");
        trace_ctx.apply(&span);
        let _guard = span.enter();

        let Ok(mut current_status) = self
            .current_status
            .lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
        else {
            return Ok(());
        };

        current_status.net_stats = Some(net_stats);

        Ok(())
    }

    fn provide_cellular_status(&self, status: CellularStatus) -> zbus::fdo::Result<()> {
        let Ok(mut current_status) = self
            .current_status
            .lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
        else {
            return Ok(());
        };

        current_status.cellular_status = Some(status);

        Ok(())
    }

    fn provide_core_stats(
        &self,
        core_stats: CoreStats,
        trace_ctx: TraceCtx,
    ) -> zbus::fdo::Result<()> {
        let span = info_span!("backend-status::provide_core_stats");
        trace_ctx.apply(&span);
        let _guard = span.enter();

        let Ok(mut current_status) = self
            .current_status
            .lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
        else {
            return Ok(());
        };

        current_status.core_stats = Some(core_stats);

        Ok(())
    }

    fn provide_signup_state(
        &self,
        signup_state: SignupState,
        trace_ctx: TraceCtx,
    ) -> zbus::fdo::Result<()> {
        let span = info_span!("backend-status::provide_signup_state");
        trace_ctx.apply(&span);
        let _guard = span.enter();

        let Ok(mut current_status) = self
            .current_status
            .lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
        else {
            return Ok(());
        };

        current_status.signup_state = Some(signup_state);

        Ok(())
    }

    fn provide_connd_report(
        &self,
        report: orb_backend_status_dbus::types::ConndReport,
    ) -> zbus::fdo::Result<()> {
        let Ok(mut current_status) = self
            .current_status
            .lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
        else {
            return Ok(());
        };

        current_status.wifi_networks = Some(report.scanned_networks.clone());
        current_status.connd_report = Some(report);

        Ok(())
    }
}

impl Default for BackendStatusImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl BackendStatusImpl {
    pub fn new() -> Self {
        Self {
            current_status: Arc::new(Mutex::new(CurrentStatus::default())),
            notify_urgent: Arc::new(Notify::new()),
            send_immediately: Arc::new(Mutex::new(false)),
        }
    }

    /// Set the urgent send flag and wake up the sender loop.
    /// The flag remains set until `clear_send_immediately()` is called
    /// after a successful send.
    pub fn set_send_immediately(&self) {
        if let Ok(mut send_immediately) = self.send_immediately.lock() {
            *send_immediately = true;
        }
        self.notify_urgent.notify_one();
    }

    /// Wait for an urgent send request. Returns immediately if an urgent
    /// event has already been signaled.
    pub async fn wait_for_urgent_send(&self) {
        self.notify_urgent.notified().await;
    }

    pub fn snapshot(&self) -> CurrentStatus {
        self.current_status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    pub fn should_send_immediately(&self) -> bool {
        self.send_immediately.lock().map(|v| *v).unwrap_or(false)
    }

    pub fn clear_send_immediately(&self) {
        if let Ok(mut send_immediately) = self.send_immediately.lock() {
            *send_immediately = false;
        }
    }

    /// Update hardware states from zenoh.
    pub fn update_hardware_states(&self, states: HashMap<String, HardwareState>) {
        let Ok(mut current_status) = self.current_status.lock() else {
            return;
        };
        if states.is_empty() {
            current_status.hardware_states = None;
        } else {
            current_status.hardware_states = Some(states);
        }
    }

    /// Update front ALS (Ambient Light Sensor) data from zenoh.
    pub fn update_front_als(&self, als: Option<AmbientLight>) {
        let Ok(mut current_status) = self.current_status.lock() else {
            return;
        };
        current_status.front_als = als;
    }

    /// Update the active SSID in the current status.
    /// Called by the connectivity watcher when SSID changes via zenoh.
    /// If connd_report doesn't exist yet, creates a minimal one.
    pub fn update_active_ssid(&self, ssid: Option<String>) {
        let Ok(mut current_status) = self.current_status.lock() else {
            return;
        };

        match &mut current_status.connd_report {
            Some(report) => {
                report.active_wifi_profile = ssid;
            }
            None => {
                // Create minimal connd_report with just the SSID
                current_status.connd_report = Some(ConndReport {
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
}
