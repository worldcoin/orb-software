pub mod types;

use orb_telemetry::TraceCtx;
use types::{LteInfo, CoreStats, NetStats, UpdateProgress, WifiNetwork};
use zbus::{fdo::Result, interface};

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

    fn provide_lte_info(&self, lte_info: LteInfo) -> Result<()>;

    fn provide_core_stats(
        &self,
        core_stats: CoreStats,
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

    fn provide_lte_info(&self, lte_info: LteInfo) -> Result<()> {
        self.0.provide_lte_info(lte_info)
    }

    fn provide_core_stats(
        &self,
        core_stats: CoreStats,
        trace_ctx: TraceCtx,
    ) -> Result<()> {
        self.0.provide_core_stats(core_stats, trace_ctx)
    }
}
