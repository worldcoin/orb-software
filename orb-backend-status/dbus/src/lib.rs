pub mod types;

use orb_telemetry::TraceCtx;
use types::{
    CellularStatus, ConndReport, CoreStats, NetStats, UpdateProgress, WifiNetwork,
};
use zbus::{fdo::Result, interface};

use crate::types::SignupState;

pub mod constants {
    pub const SERVICE_NAME: &str = "org.worldcoin.BackendStatus1";
    pub const OBJECT_PATH: &str = "/org/worldcoin/BackendStatus1";
    pub const INTERFACE_NAME: &str = "org.worldcoin.BackendStatus1";
}

pub trait BackendStatusT: Send + Sync + 'static {
    fn provide_wifi_networks(
        &self,
        wifi_networks: Vec<WifiNetwork>,
        trace_ctx: TraceCtx,
    ) -> Result<()>;

    fn provide_update_progress(
        &self,
        update_progress: UpdateProgress,
        trace_ctx: TraceCtx,
    ) -> Result<()>;

    fn provide_net_stats(&self, net_stats: NetStats, trace_ctx: TraceCtx)
        -> Result<()>;

    fn provide_cellular_status(&self, status: CellularStatus) -> Result<()>;

    fn provide_connd_report(&self, report: ConndReport) -> Result<()>;

    fn provide_core_stats(
        &self,
        core_stats: CoreStats,
        trace_ctx: TraceCtx,
    ) -> Result<()>;

    fn provide_signup_state(
        &self,
        signup_state: SignupState,
        trace_ctx: TraceCtx,
    ) -> Result<()>;
}

#[derive(Debug, derive_more::From)]
pub struct BackendStatus<T>(pub T);

#[interface(
    name = "org.worldcoin.BackendStatus1",
    proxy(
        default_service = "org.worldcoin.BackendStatus1",
        default_path = "/org/worldcoin/BackendStatus1",
    )
)]
impl<T: BackendStatusT> BackendStatusT for BackendStatus<T> {
    fn provide_wifi_networks(
        &self,
        wifi_networks: Vec<WifiNetwork>,
        trace_ctx: TraceCtx,
    ) -> Result<()> {
        self.0.provide_wifi_networks(wifi_networks, trace_ctx)
    }

    fn provide_update_progress(
        &self,
        update_progress: UpdateProgress,
        trace_ctx: TraceCtx,
    ) -> Result<()> {
        self.0.provide_update_progress(update_progress, trace_ctx)
    }

    fn provide_net_stats(
        &self,
        net_stats: NetStats,
        trace_ctx: TraceCtx,
    ) -> Result<()> {
        self.0.provide_net_stats(net_stats, trace_ctx)
    }

    fn provide_cellular_status(&self, status: CellularStatus) -> Result<()> {
        self.0.provide_cellular_status(status)
    }

    fn provide_connd_report(&self, report: ConndReport) -> Result<()> {
        self.0.provide_connd_report(report)
    }

    fn provide_core_stats(
        &self,
        core_stats: CoreStats,
        trace_ctx: TraceCtx,
    ) -> Result<()> {
        self.0.provide_core_stats(core_stats, trace_ctx)
    }

    fn provide_signup_state(
        &self,
        signup_state: SignupState,
        trace_ctx: TraceCtx,
    ) -> Result<()> {
        self.0.provide_signup_state(signup_state, trace_ctx)
    }
}
