use orb_backend_status_dbus::{
    types::{
        CellularStatus, ConndReport, CoreStats, NetStats, SignupState, UpdateProgress,
        WifiNetwork,
    },
    BackendStatusT,
};

use orb_telemetry::TraceCtx;
use orb_update_agent_dbus::UpdateAgentState;
use std::{
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;
use tracing::{error, info_span};

#[derive(Clone)]
pub struct BackendStatusImpl {
    current_status: Arc<Mutex<CurrentStatus>>,
    changed: Arc<Mutex<bool>>,
    notify: Arc<Notify>,
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

        let Ok(mut current_status) = self.current_status.lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
        else {
            return Ok(());
        };

        current_status.update_progress = Some(update_progress);

        // Urgent: if update-agent is rebooting, flush immediately.
        if current_status
            .update_progress
            .as_ref()
            .is_some_and(|p| p.state == UpdateAgentState::Rebooting)
            && let Ok(mut send_immediately) = self.send_immediately.lock()
        {
            *send_immediately = true;
        }

        if let Ok(mut changed) = self.changed.lock() {
            *changed = true;
        }
        self.notify.notify_one();

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

        let Ok(mut current_status) = self.current_status.lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
        else {
            return Ok(());
        };

        current_status.net_stats = Some(net_stats);
        if let Ok(mut changed) = self.changed.lock() {
            *changed = true;
        }
        self.notify.notify_one();

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
        if let Ok(mut changed) = self.changed.lock() {
            *changed = true;
        }
        self.notify.notify_one();

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

        let Ok(mut current_status) = self.current_status.lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
        else {
            return Ok(());
        };

        current_status.core_stats = Some(core_stats);
        if let Ok(mut changed) = self.changed.lock() {
            *changed = true;
        }
        self.notify.notify_one();

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

        let Ok(mut current_status) = self.current_status.lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
        else {
            return Ok(());
        };

        current_status.signup_state = Some(signup_state);
        if let Ok(mut changed) = self.changed.lock() {
            *changed = true;
        }

        self.notify.notify_one();

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

        // Urgent only when SSID changes (active_wifi_profile).
        let prev_active = current_status
            .connd_report
            .as_ref()
            .and_then(|r| r.active_wifi_profile.clone());
        let next_active = report.active_wifi_profile.clone();

        current_status.wifi_networks = Some(report.scanned_networks.clone());
        current_status.connd_report = Some(report);

        if let Ok(mut changed) = self.changed.lock() {
            *changed = true;
        }

        if prev_active != next_active
            && let Ok(mut send_immediately) = self.send_immediately.lock()
        {
            *send_immediately = true;
        }

        self.notify.notify_one();

        Ok(())
    }
}

impl BackendStatusImpl {
    pub fn new() -> Self {
        Self {
            current_status: Arc::new(Mutex::new(CurrentStatus::default())),
            changed: Arc::new(Mutex::new(false)),
            notify: Arc::new(Notify::new()),
            send_immediately: Arc::new(Mutex::new(false)),
        }
    }

    pub async fn wait_for_change(&self) {
        self.notify.notified().await;
    }

    pub fn snapshot(&self) -> CurrentStatus {
        self.current_status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    pub fn changed(&self) -> bool {
        self.changed.lock().map(|v| *v).unwrap_or(false)
    }

    pub fn clear_changed(&self) {
        if let Ok(mut changed) = self.changed.lock() {
            *changed = false;
        }
    }

    pub fn should_send_immediately(&self) -> bool {
        self.send_immediately.lock().map(|v| *v).unwrap_or(false)
    }

    pub fn clear_send_immediately(&self) {
        if let Ok(mut send_immediately) = self.send_immediately.lock() {
            *send_immediately = false;
        }
    }
}

