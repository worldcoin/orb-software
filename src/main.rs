use std::{convert::Infallible, str::FromStr};

use eyre::WrapErr as _;
use tracing::{info, metadata::LevelFilter};

const TELEPORT_METRICS_URL: &str = "http://127.0.0.1:3000/readyz";

mod telemetry;
use crate::telemetry::ExecContext;

enum TeleportStatus {
    Ok,
    Other(String),
}

impl FromStr for TeleportStatus {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ok" => Ok(Self::Ok),
            other => Ok(Self::Other(other.into())),
        }
    }
}

fn get_teleport_status() -> eyre::Result<TeleportStatus> {
    #[derive(Debug, serde::Deserialize)]
    struct TeleportMetricsBody {
        status: String,
    }
    let resp = reqwest::blocking::get(TELEPORT_METRICS_URL)
        .wrap_err_with(|| format!("unable to query {TELEPORT_METRICS_URL}"))?;
    let body: TeleportMetricsBody = resp
        .json()
        .wrap_err_with(|| format!("failed reading {TELEPORT_METRICS_URL} response"))?;
    Ok(TeleportStatus::from_str(&body.status.trim().to_lowercase()).unwrap())
}

fn main() -> eyre::Result<()> {
    telemetry::start::<ExecContext, _>(LevelFilter::INFO, std::io::stdout)
        .wrap_err("failed initializing tracing")?;

    if slot_ctrl::get_current_rootfs_status()?.is_update_done() {
        match get_teleport_status().wrap_err("failed querying teleport daemon for status")? {
            TeleportStatus::Ok => info!("teleport status is OK, continuing"),
            TeleportStatus::Other(msg) => {
                info!("teleport returned a status other than OK; aborting. teleport msg: {msg}");
                return Ok(());
            }
        }
        info!("setting rootfs status to Normal to confirm the last update");
        slot_ctrl::set_current_rootfs_status(slot_ctrl::RootFsStatus::Normal)?;
    }

    info!("setting retry counter to maximum for future boot attempts");
    slot_ctrl::reset_current_retry_count_to_max()?;
    Ok(())
}
