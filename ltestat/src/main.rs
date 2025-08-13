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

    println!("Getting the modem id");

    let output = run_cmd("mmcli", &["-L"]).await?;
    let modem_id = output
        .split_whitespace()
        .next()
        .and_then(|path| path.rsplit('/').next())
        .ok_or_eyre("Failed to get modem id")?
        .to_owned();

    println!("Modem id: {}", modem_id);

    println!("Starting LTE telemetry logger...");

    let _ = utils::run_cmd("mmcli", &["-m", &modem_id, "--signal-setup", "10"]).await;

    let mut mon = ModemMonitor::new(modem_id);

    loop {
        let now_inst = Instant::now();
        let now_utc = Utc::now();

        // update state (no logs)
        let state = match ConnectionState::get_connection_state(&mon.modem_id).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Connection state error: {e}");
                ConnectionState::Unknown
            }
        };
        mon.update_state(now_inst, now_utc, state);

        // poll snapshot (optional to print or persist)
        match mon.poll_lte().await {
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
