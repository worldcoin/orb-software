use color_eyre::eyre::{Result, WrapErr};
use orb_info::orb_os_release::{OrbOsPlatform, OrbOsRelease, OrbRelease};
use std::time::Duration;
use tokio::{
    fs,
    signal::unix::{self, SignalKind},
    task::JoinHandle,
};
use tracing::{info, warn};
use utils::retry_for;

mod cellular;
mod modem_manager;
mod telemetry;
mod utils;

pub type Tasks = Vec<JoinHandle<Result<()>>>;

pub async fn run(os_release: OrbOsRelease) -> Result<()> {
    // TODO: this is temporary while this daemon only supports cellular metrics
    // Once there is more logic added relating to WiFi and Bluetooth we should remove this check
    if let OrbOsPlatform::Pearl = os_release.orb_os_platform_type {
        warn!("Cellular is not supported on Pearl. Exiting");
        return Ok(());
    }

    info!("checking if modem exists");
    if let Err(e) = retry_for(
        Duration::from_secs(30),
        Duration::from_secs(5),
        wwan0_exists,
    )
    .await
    {
        warn!("{e}, assuming this orb does not have a modem and quitting the application.");
        return Ok(());
    }

    let mut tasks = vec![];
    if os_release.release_type != OrbRelease::Service {
        tasks.push(
            cellular::start(Duration::from_secs(30), Duration::from_secs(20)).await,
        )
    }

    tasks.extend(telemetry::start().await?);

    let mut sigterm = unix::signal(SignalKind::terminate())?;
    let mut sigint = unix::signal(SignalKind::interrupt())?;

    tokio::select! {
        _ = sigterm.recv() => warn!("received SIGTERM"),
        _ = sigint.recv()  => warn!("received SIGINT"),
    }

    info!("aborting tasks and exiting gracefully");

    for handle in tasks {
        handle.abort();
    }

    Ok(())
}

async fn wwan0_exists() -> Result<bool> {
    fs::metadata("/sys/class/net/wwan0")
        .await
        .map(|_| true)
        .inspect_err(|e| warn!("wwan0 does not seem to exist: {e}"))
        .wrap_err("/sys/class/net/wwan0 does not exist")
}
