use color_eyre::eyre::{ContextCompat, Result};
use tokio::time::{sleep, Duration};

mod connection_state;
mod dd_handler;
mod lte_data;
mod modem;
mod modem_manager;
mod modem_monitor;
mod utils;

use crate::modem::Modem;
use crate::modem_monitor::start;
use crate::utils::State;
use orb_info::orb_os_release::{OrbOsPlatform, OrbOsRelease};

#[tokio::main]
async fn main() -> Result<()> {
    if let OrbOsPlatform::Pearl = OrbOsRelease::read().await?.orb_os_platform_type {
        println!("LTE is not supported on Pearl. Exiting");
        return Ok(());
    }

    let dd = dd_handler::Telemetry::new()
        .wrap_err("Failed to initialize DataDog. Exiting")?;

    let modem_info = modem_manager::get_modem_info().await?;
    let modem = Modem::new(modem_info.modem_id, modem_info.iccid, modem_info.imei)?;
    let modem = State::new(modem);
    let poll_interval = Duration::from_secs(30);
    let modem_poll_handle = modem_monitor::start(modem.clone(), poll_interval);

    todo!();

    println!(
        "Current imei: {}, current iccid: {}",
        monitor.imei, monitor.iccid
    );

    if let Err(e) = monitor.wait_for_connection().await {
        eprintln!("wait_for_connection error: {e}");
    }

    println!(
        "Current rat: {:?}, current operator: {:?}",
        monitor.rat, monitor.operator
    );

    let modem_id = monitor.modem_id.clone();
    loop {
        if !monitor.state.is_online() {
            if let Err(e) = monitor.wait_for_connection().await {
                eprintln!("wait_for_connection error: {e}");
            } else {
                dd.incr_reconnect(&monitor.modem_id);
                if let Some(dt) = monitor.last_downtime_secs {
                    dd.gauge_reconnect_time(&monitor.modem_id, dt);
                }
            }
        }

        match monitor.poll_lte().await {
            Ok(snapshot) => {
                dd.on_poll_success(&modem_id, snapshot);
            }
            Err(_) => dd.on_poll_error(&modem_id, monitor.state),
        }

        sleep(Duration::from_secs(30)).await;
    }
}
