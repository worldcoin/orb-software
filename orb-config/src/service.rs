use std::time::Duration;

use orb_info::OrbId;
use reqwest::{Client, Url};
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{Map, Value};
use tracing::{info, warn};

use crate::{
    backend::{self, Fetch, Snapshot},
    publish,
};

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Deserialize)]
struct ConfigDaemonConfig {
    poll_interval: Duration,
}

pub async fn run(
    client: Client,
    endpoint: Url,
    orb_id: OrbId,
    token: SecretString,
    mut publisher: publish::Publisher,
) {
    let mut cached: Option<Snapshot> = None;
    let mut poll_interval = DEFAULT_POLL_INTERVAL;

    loop {
        let mut delay = poll_interval;
        match backend::fetch(&client, &endpoint, &orb_id, &token, cached.as_ref()).await
        {
            Ok(Fetch::Updated(snapshot)) => {
                let interval = configured_interval(&snapshot.sections, poll_interval);
                if interval != poll_interval {
                    info!(seconds = interval.as_secs_f64(), "poll interval changed");
                }
                poll_interval = interval;
                delay = interval;
                cached = Some(snapshot);
            }
            Ok(Fetch::Unchanged) => {}
            Ok(Fetch::RetryAfter(retry_after)) => delay = delay.max(retry_after),
            Err(error) => {
                warn!(%error, "configuration fetch failed; retrying next poll")
            }
        }

        // Retry failed publications even after a 304 or HTTP failure.
        if let Some(snapshot) = &cached {
            publish::publish(&mut publisher, snapshot.sections.clone());
        }
        tokio::time::sleep(delay).await;
    }
}

fn configured_interval(sections: &Map<String, Value>, previous: Duration) -> Duration {
    let Some(section) = sections.get("config") else {
        return previous;
    };
    match serde_json::from_value::<ConfigDaemonConfig>(section.clone()) {
        Ok(config)
            if (Duration::from_secs(1)..=Duration::from_secs(86_400))
                .contains(&config.poll_interval) =>
        {
            config.poll_interval
        }
        _ => {
            warn!("invalid config.poll_interval; keeping the previous interval (allowed: 1s to 24h)");
            previous
        }
    }
}
