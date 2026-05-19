use async_trait::async_trait;
use color_eyre::Result;
use dogstatsd::DogstatsdResult;
use flume::Sender;
use std::{fs, path::Path, thread, time::Duration};
use tokio::sync::oneshot;
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
                let opts = if fs::exists(Path::new(DOGSTATSD_SOCKET_PATH))
                    .unwrap_or(false)
                {
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
                    Msg::Count {
                        stat,
                        count,
                        tags,
                        reply,
                    } => {
                        let _ = reply.send(client.count(stat, count, tags));
                    }

                    Msg::Gauge {
                        stat,
                        val,
                        tags,
                        reply,
                    } => {
                        let _ = reply.send(client.gauge(stat, val, tags));
                    }

                    Msg::IncrByValue {
                        stat,
                        value,
                        tags,
                        reply,
                    } => {
                        let _ = reply.send(client.incr_by_value(stat, value, tags));
                    }
                }
            }
        });

        Self { tx }
    }
}

#[async_trait]
impl StatsdClient for DogstatsdClient {
    async fn count(&self, stat: &str, count: i64, tags: Vec<String>) -> Result<()> {
        let (reply, rx) = oneshot::channel();

        self.tx.send(Msg::Count {
            stat: stat.to_string(),
            count,
            tags,
            reply,
        })?;

        Ok(rx.await??)
    }

    async fn incr_by_value(
        &self,
        stat: &str,
        value: i64,
        tags: Vec<String>,
    ) -> Result<()> {
        let (reply, rx) = oneshot::channel();

        self.tx.send(Msg::IncrByValue {
            stat: stat.to_string(),
            value,
            tags,
            reply,
        })?;

        Ok(rx.await??)
    }

    async fn gauge(&self, stat: &str, val: &str, tags: Vec<String>) -> Result<()> {
        let (reply, rx) = oneshot::channel();

        self.tx.send(Msg::Gauge {
            stat: stat.to_string(),
            val: val.to_string(),
            tags,
            reply,
        })?;

        Ok(rx.await??)
    }
}

enum Msg {
    Count {
        stat: String,
        count: i64,
        tags: Vec<String>,
        reply: oneshot::Sender<DogstatsdResult>,
    },

    Gauge {
        stat: String,
        val: String,
        tags: Vec<String>,
        reply: oneshot::Sender<DogstatsdResult>,
    },

    IncrByValue {
        stat: String,
        value: i64,
        tags: Vec<String>,
        reply: oneshot::Sender<DogstatsdResult>,
    },
}
