use color_eyre::Result;
use tokio::time::{sleep, Duration};

mod connection_state;
mod lte_data;
mod modem_monitor;
mod utils;

use crate::modem_monitor::ModemMonitor;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let mut monitor = ModemMonitor::new().await?;

    // Loops every 10 seconds untill we get a connection from LTE
    monitor.wait_for_connection().await?;

    loop {
        if !monitor.state.is_online() {
            monitor.wait_for_connection().await?;
        }

        match monitor.poll_lte().await {
            Ok(snapshot) => {
                if let Ok(json) = serde_json::to_string_pretty(snapshot) {
                    println!("{json}\n");
                }
            }
            Err(e) => eprintln!("Polling error: {e:?}"),
        }

        sleep(Duration::from_secs(30)).await;
    }
}
