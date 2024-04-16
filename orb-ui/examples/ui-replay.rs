use chrono::{DateTime, Utc};
use clap::Parser;
use eyre::{Context, ContextCompat, Result};
use std::fs::File;
use std::io;
use std::io::BufRead;
use std::str::FromStr;
use tokio::time::sleep;
use tracing::level_filters::LevelFilter;
use tracing::{debug, info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};
use zbus::Connection;

const RECORDS_FILE: &str = "worldcoin-ui-logs.txt";

#[zbus::proxy(
    default_service = "org.worldcoin.OrbUiState1",
    default_path = "/org/worldcoin/OrbUiState1",
    interface = "org.worldcoin.OrbUiState1"
)]
trait SignupState {
    fn orb_signup_state_event(&self, serialized_event: String) -> zbus::Result<()>;
}

/// Utility args
#[derive(clap::Parser, Debug)]
#[clap(
    author,
    version,
    about = "Orb UI replay tool",
    long_about = "Replay events from a records file to orb-ui over dbus"
)]
struct Args {
    #[clap(short, long)]
    path: Option<String>,
}

#[derive(Debug, Default)]
struct EventRecord {
    timestamp: DateTime<Utc>,
    event: String,
}

impl FromStr for EventRecord {
    type Err = eyre::Report;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = line.split(' ').collect();
        if parts.len() < 2 {
            return Err(eyre::eyre!("Line is too short"));
        }
        let timestamp_str = parts[5];

        // split line to take everything after "UI event:"
        let (_, event) = line
            .split_once("UI event: ")
            .wrap_err("Unable to split line")?;
        let event = event.to_string();
        match timestamp_str.parse::<DateTime<Utc>>() {
            Ok(timestamp) => {
                debug!("Timestamp: {:?}, Event: {:?}", timestamp, event);
                Ok(EventRecord { timestamp, event })
            }
            Err(error) => Err(eyre::eyre!("Unable to parse timestamp: {error}")),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let args = Args::parse();
    let connection = Connection::session().await?;
    let proxy = SignupStateProxy::new(&connection).await?;

    // get path to records file from program arguments or use default
    let path = args.path.unwrap_or(RECORDS_FILE.to_string());
    let file =
        File::open(path.clone()).wrap_err_with(|| format!("cannot open {path}"))?;
    let reader = io::BufReader::new(file);

    let mut last_timestamp: Option<DateTime<Utc>> = None;
    for record in reader.lines().map(|line| line?.parse::<EventRecord>()) {
        let record = record?;
        if let Some(last) = last_timestamp {
            let delay = record.timestamp - last;
            sleep(delay.to_std().unwrap()).await;
        }

        let event = record.event;
        info!("Sending: {}", event);
        // send the event to orb-ui over dbus
        if let Err(e) = proxy.orb_signup_state_event(format!("{event}")).await {
            warn!("Error sending event {event}: {:?}", e);
        }

        last_timestamp = Some(record.timestamp);
    }

    Ok(())
}
