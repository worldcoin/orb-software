use crate::dbus::intf_impl::BackendStatusImpl;
use orb_backend_status_dbus::{
    types::{
        CellularStatus, ConndReport, CoreStats, NetStats, SignupState, UpdateProgress,
        WifiNetwork,
    },
    BackendStatusT,
};
use orb_telemetry::TraceCtx;
use orb_update_agent_dbus::UpdateAgentState;
use tracing::{error, info_span};

/// THIS IS DEPRECATED, PLEASE DO NOT ADD ANY NEW FIELDS OR USE THIS
/// ANYMORE. If you need to send new data types to the backend, use the
/// OES.
#[derive(Debug, Default, Clone)]
pub struct DbusStatus {
    pub wifi_networks: Option<Vec<WifiNetwork>>,
    pub update_progress: Option<UpdateProgress>,
    pub net_stats: Option<NetStats>,
    pub cellular_status: Option<CellularStatus>,
    pub core_stats: Option<CoreStats>,
    pub signup_state: Option<SignupState>,
    pub connd_report: Option<ConndReport>,
}

/// THIS IS DEPRECATED, PLEASE DO NOT ADD ANY NEW METHODS OR USE THIS ANYMORE
/// If you need to send new data types to the backend, use the OES
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

        current_status.dbus.update_progress = Some(update_progress);

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

        current_status.dbus.net_stats = Some(net_stats);

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

        current_status.dbus.cellular_status = Some(status);

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

        current_status.dbus.core_stats = Some(core_stats);

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

        current_status.dbus.signup_state = Some(signup_state);

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

        current_status.dbus.wifi_networks = Some(report.scanned_networks.clone());
        current_status.dbus.connd_report = Some(report);

        Ok(())
    }
}
