use crate::{
    network_manager::NetworkManager, resolved::Resolved, statsd::StatsdClient,
};
use color_eyre::Result;
use speare::{mini::OnErr, Backoff, Limit};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tracing::info;

pub mod active_connections;
pub mod cellular_status;
pub mod connd_report;
pub mod datadog;
pub mod net_stats;

#[allow(clippy::too_many_arguments)]
pub async fn spawn(
    speare: &speare::mini::Ctx<()>,
    nm: NetworkManager,
    resolved: Resolved,
    session_bus: zbus::Connection,
    statsd: Arc<dyn StatsdClient>,
    zsender: zenorb::Sender,
    sysfs: PathBuf,
    procfs: PathBuf,
) -> Result<()> {
    info!("starting reporter tasks");

    speare
        .task_with()
        .args(cellular_status::Args {
            dbus: session_bus.clone(),
            zsender: zsender.clone(),
        })
        .on_err(static_backoff(15))
        .spawn(cellular_status::report)?;

    speare
        .task_with()
        .args(net_stats::Args {
            poll_interval: Duration::from_secs(30),
            sysfs: sysfs.clone(),
            zsender: zsender.clone(),
        })
        .on_err(static_backoff(15))
        .spawn(net_stats::report)?;

    speare
        .task_with()
        .args(datadog::Args { statsd })
        .on_err(static_backoff(15))
        .spawn(datadog::report)?;

    speare
        .task_with()
        .args(connd_report::Args {
            nm: nm.clone(),
            session_bus,
            report_interval: Duration::from_secs(30),
        })
        .on_err(static_backoff(15))
        .spawn(connd_report::report)?;

    speare
        .task_with()
        .args(active_connections::Args {
            nm,
            resolved,
            zsender,
            sysfs,
            procfs,
        })
        .on_err(static_backoff(15))
        .spawn(active_connections::report)?;

    Ok(())
}

fn static_backoff(seconds: u64) -> OnErr {
    OnErr::Restart {
        max: Limit::None,
        backoff: Backoff::Static(Duration::from_secs(seconds)),
    }
}
