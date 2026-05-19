use color_eyre::Result;
use flume::Sender;
use std::{fs, path::Path, thread, time::Duration};
use tracing::{error, info, warn};

use super::StatsdClient;

const DOGSTATSD_SOCKET_PATH: &str = "/run/datadog/dsd.socket";

pub struct DogstatsdClient {
    tx: Sender<Msg>,
}

impl DogstatsdClient {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let backoff = Duration::from_secs(10);
        let (tx, rx) = flume::unbounded();

        thread::spawn(move || {
            let client = loop {
                let opts =
                    if fs::exists(Path::new(DOGSTATSD_SOCKET_PATH)).unwrap_or(false) {
                        info!("datadog-agent socket found, using it for IPC");

                        dogstatsd::OptionsBuilder::new()
                            .socket_path(Some(DOGSTATSD_SOCKET_PATH.to_string()))
                            .build()
                    } else {
                        warn!(
                        "datadog-agent socket not found, falling back to UDP for IPC"
                    );

                        dogstatsd::Options::default()
                    };

                match dogstatsd::Client::new(opts) {
                    Ok(client) => break client,
                    Err(e) => {
                        error!(
                            "failed to create dd client: {e}, trying again in {}s",
                            backoff.as_secs()
                        );

                        thread::sleep(backoff);
                    }
                }
            };

            while let Ok(msg) = rx.recv() {
                match msg {
                    Msg::Count { stat, count, tags } => {
                        let _ = client
                            .count(&stat, count, tags)
                            .inspect_err(|e| warn!("failed to count {stat}. err: {e}"));
                    }

                    Msg::Gauge { stat, val, tags } => {
                        let _ = client
                            .gauge(&stat, val, tags)
                            .inspect_err(|e| warn!("failed to gauge {stat}. err: {e}"));
                    }

                    Msg::IncrByValue { stat, value, tags } => {
                        let _ =
                            client.incr_by_value(&stat, value, tags).inspect_err(|e| {
                                warn!("failed to incr_by_value {stat}. err: {e}")
                            });
                    }

                    Msg::Distribution { stat, val, tags } => {
                        let _ =
                            client.distribution(&stat, val, tags).inspect_err(|e| {
                                warn!("failed to distribution {stat}. err: {e}")
                            });
                    }
                }
            }
        });

        Self { tx }
    }
}

impl StatsdClient for DogstatsdClient {
    fn count(&self, stat: &str, count: i64, tags: Vec<String>) -> Result<()> {
        self.tx.send(Msg::Count {
            stat: stat.to_string(),
            count,
            tags,
        })?;

        Ok(())
    }

    fn incr_by_value(&self, stat: &str, value: i64, tags: Vec<String>) -> Result<()> {
        self.tx.send(Msg::IncrByValue {
            stat: stat.to_string(),
            value,
            tags,
        })?;

        Ok(())
    }

    fn gauge(&self, stat: &str, val: &str, tags: Vec<String>) -> Result<()> {
        self.tx.send(Msg::Gauge {
            stat: stat.to_string(),
            val: val.to_string(),
            tags,
        })?;

        Ok(())
    }

    fn distribution(&self, stat: &str, val: &str, tags: Vec<String>) -> Result<()> {
        self.tx.send(Msg::Distribution {
            stat: stat.to_string(),
            val: val.to_string(),
            tags,
        })?;

        Ok(())
    }
}

enum Msg {
    Count {
        stat: String,
        count: i64,
        tags: Vec<String>,
    },

    Gauge {
        stat: String,
        val: String,
        tags: Vec<String>,
    },

    IncrByValue {
        stat: String,
        value: i64,
        tags: Vec<String>,
    },

    Distribution {
        stat: String,
        val: String,
        tags: Vec<String>,
    },
}
