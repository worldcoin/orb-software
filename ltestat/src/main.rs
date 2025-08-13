use std::time::Instant;

use chrono::Utc;
use color_eyre::Result;
use connection_state::ConnectionState;
use tokio::time::{sleep, Duration};

mod connection_state;
mod lte_data;
mod utils;

use lte_data::LteStat;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    println!("Starting LTE telemetry logger...");

    // This is needed to force mmcli to log signal data every N(10) seconds
    let _ = utils::run_cmd("sudo", &["mmcli", "-m", "0", "--signal-setup 10"]).await;

    let mut last_state: Option<ConnectionState> = None;
    let mut disconnected_since: Option<Instant> = None;
    let mut disconnected_count = 0;

    loop {
        let now_inst = Instant::now();
        let now_utc = Utc::now();

        let state = match ConnectionState::get_connection_state().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Connection state error: {e}");
                ConnectionState::Unknown
            }
        };

        println!("Connection state: {:?}", state);

        let was_connected = last_state.as_ref().is_some_and(|s| s.is_online());
        let is_connected = state.is_online();

        if was_connected && !is_connected {
            disconnected_since = Some(now_inst);
            disconnected_count += 1;

            println!(
                "Disconnected at {}. Total number of disconnects: {}",
                now_utc.to_rfc3339(),
                disconnected_count
            );
        } else if !was_connected && is_connected {
            if let Some(start) = disconnected_since.take() {
                let secs = now_inst.duration_since(start).as_secs_f64();
                println!(
                    "Reconnected at {}. Downtime {}. Total number of disconnects: {}",
                    now_utc.to_rfc3339(),
                    secs,
                    disconnected_count
                );
            }
        }

        last_state = Some(state);

        match LteStat::poll().await {
            Ok(snapshot) => {
                // Pretty-print JSON to stdout
                match serde_json::to_string_pretty(&snapshot) {
                    Ok(json) => println!("{json}\n"),
                    Err(e) => eprintln!("Failed to serialize snapshot: {e:?}"),
                }
            }
            Err(e) => {
                eprintln!("Polling error: {e:?}");
            }
        }

        sleep(Duration::from_secs(30)).await;
    }
}
