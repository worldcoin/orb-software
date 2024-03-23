use chrono::{DateTime, Utc};
use std::fs::File;
use std::io;
use std::io::BufRead;
use tokio::time::sleep;
use tracing::level_filters::LevelFilter;
use tracing::{debug, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};
use zbus::Connection;
use eyre::Result;

#[zbus::proxy(
    default_service = "org.worldcoin.OrbSignupState1",
    default_path = "/org/worldcoin/OrbSignupState1",
    interface = "org.worldcoin.OrbSignupState1"
)]
trait SignupState {
    fn orb_signup_state_event(&self, serialized_event: String) -> zbus::Result<String>;
}

fn parse_line(line: &str) -> Option<(DateTime<Utc>, &str)> {
    let parts: Vec<&str> = line.split(' ').collect();
    if parts.len() < 2 {
        return None;
    }
    let timestamp_str = parts[5];

    // split line to take everything after "UI event:"
    let (_, event) = line.split_once("UI event: ")?;
    debug!("Timestamp: {}, Event: {}", timestamp_str, event);

    match timestamp_str.parse::<DateTime<Utc>>() {
        Ok(timestamp) => Some((timestamp, event)),
        Err(_) => None,
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

    let connection = Connection::session().await?;
    let proxy = SignupStateProxy::new(&connection).await?;

    // set initial state
    let _ = proxy.orb_signup_state_event(format!("\"Bootup\"")).await;
    let _ = proxy.orb_signup_state_event(format!("\"Idle\"")).await;
    let _ = proxy
        .orb_signup_state_event(format!("\"SoundVolume {{ level: 10 }}\""))
        .await;

    // get path to records file from program arguments or use default
    let path = std::env::args().nth(1).unwrap_or("records.txt".to_string());
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);

    let mut last_timestamp: Option<DateTime<Utc>> = None;

    for line in reader.lines() {
        let line = line?;
        if let Some((timestamp, event)) = parse_line(&line) {
            if let Some(last) = last_timestamp {
                let delay = timestamp - last;
                sleep(delay.to_std().unwrap()).await;
            }

            info!("Sending: {}", event);
            // send the event to orb-ui over dbus
            let _ = proxy.orb_signup_state_event(format!("\"{event}\"")).await;

            last_timestamp = Some(timestamp);
        }
    }

    Ok(())
}
