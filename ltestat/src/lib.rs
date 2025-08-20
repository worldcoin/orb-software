use color_eyre::eyre::Result;
use modem::{modem_manager, Modem};
use orb_info::orb_os_release::{OrbOsPlatform, OrbOsRelease};
use std::time::Duration;
use tokio::time::{self, Instant};
use utils::State;

mod dd_handler;
mod metrics_reporter;
mod modem;
mod modem_monitor;
mod utils;

pub async fn run() -> Result<()> {
    if let OrbOsPlatform::Pearl = OrbOsRelease::read().await?.orb_os_platform_type {
        println!("LTE is not supported on Pearl. Exiting");
        return Ok(());
    }

    let modem =
        try_make_modem(Duration::from_secs(120), Duration::from_secs(10)).await?;

    let modem = State::new(modem);

    let monitor_handle = modem_monitor::start(modem.clone(), Duration::from_secs(20));
    let reporter_handle = metrics_reporter::start(modem, Duration::from_secs(30));

    tokio::select! {
        _ = monitor_handle => {}
        _ = reporter_handle => {}
    }

    Ok(())
}

async fn try_make_modem(timeout: Duration, backoff: Duration) -> Result<Modem> {
    let start = Instant::now();

    loop {
        let modem = async {
            let modem_id = modem_manager::get_modem_id().await?;
            let imei = modem_manager::get_imei(&modem_id).await?;
            let iccid = modem_manager::get_iccid().await?;
            let state = modem_manager::get_connection_state(&modem_id).await?;
            modem_manager::start_signal_refresh(&modem_id).await?;

            Ok(Modem::new(modem_id, iccid, imei, state))
        }
        .await;

        match modem {
            Err(e) => {
                if start.elapsed() >= timeout {
                    return Err(e);
                }

                time::sleep(backoff).await;
            }

            Ok(m) => return Ok(m),
        }
    }
}
