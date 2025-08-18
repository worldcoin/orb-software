use tokio::time::{sleep, Duration};

mod connection_state;
mod lte_data;
mod modem_monitor;
mod utils;

use crate::modem_monitor::ModemMonitor;

#[tokio::main]
async fn main() {
    // Loops every 10 seconds untill we get a connection from LTE
    let mut monitor = match ModemMonitor::new().await {
        Ok(m) => m,
        Err(_) => {
            println!("This Orb does not have a modem. Exiting.");
            return;
        }
    };

    if let Err(e) = monitor.wait_for_connection().await {
        eprintln!("wait_for_connection error: {e}");
    }
    loop {
        if !monitor.state.is_online() {
            if let Err(e) = monitor.wait_for_connection().await {
                eprintln!("wait_for_connection error: {e}");
            }
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
