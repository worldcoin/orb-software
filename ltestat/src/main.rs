use color_eyre::Result;
use tokio::time::{sleep, Duration};

mod lte_data;
mod utils;

use lte_data::LteStat;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    println!("Starting LTE telemetry logger...");

    loop {
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
