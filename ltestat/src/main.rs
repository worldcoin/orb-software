use chrono::Utc;
use color_eyre::{eyre::OptionExt, Result};
use connection_state::ConnectionState;
use tokio::time::{sleep, Duration, Instant};

mod connection_state;
mod lte_data;
mod utils;

use lte_data::ModemMonitor;
use utils::run_cmd;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    println!("Starting LTE telemetry logger...");

    let mut monitor = ModemMonitor::new().await?;

    let _ = utils::run_cmd("mmcli", &["-m", &monitor.modem_id, "--signal-setup", "10"])
        .await;

    loop {
        let now_inst = Instant::now();
        let now_utc = Utc::now();

        // update state (no logs)
        let state = match ConnectionState::get_connection_state(&monitor.modem_id).await
        {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Connection state error: {e}");
                ConnectionState::Unknown
            }
        };
        monitor.update_state(now_inst, now_utc, state);
        match monitor.state.is_online() {
            true => {}
            false => monitor.dump_info(),
        }

        // poll snapshot (optional to print or persist)
        match monitor.poll_lte().await {
            Ok(snapshot) => {
                // keep printing snapshot if you still want stdout JSON
                if let Ok(json) = serde_json::to_string_pretty(snapshot) {
                    println!("{json}\n");
                }
            }
            Err(e) => eprintln!("Polling error: {e:?}"),
        }

        sleep(Duration::from_secs(30)).await;
    }
}
