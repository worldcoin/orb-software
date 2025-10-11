use color_eyre::Result;
use dogstatsd::DogstatsdResult;
use flume::Sender;
use std::{thread, time::Duration};
use tokio::sync::oneshot;
use tracing::error;

use super::StatsdClient;

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
                match dogstatsd::Client::new(dogstatsd::Options::default()) {
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

impl StatsdClient for DogstatsdClient {
    async fn count<S: AsRef<str> + Sync + Send>(
        &self,
        stat: &str,
        count: i64,
        tags: &[S],
    ) -> Result<()> {
        let (reply, rx) = oneshot::channel();

        self.tx.send(Msg::Count {
            stat: stat.to_string(),
            count,
            tags: tags.iter().map(|x| x.as_ref().to_string()).collect(),
            reply,
        })?;

        Ok(rx.await??)
    }

    async fn incr_by_value<S: AsRef<str> + Sync + Send>(
        &self,
        stat: &str,
        value: i64,
        tags: &[S],
    ) -> Result<()> {
        let (reply, rx) = oneshot::channel();

        self.tx.send(Msg::IncrByValue {
            stat: stat.to_string(),
            value,
            tags: tags.iter().map(|x| x.as_ref().to_string()).collect(),
            reply,
        })?;

        Ok(rx.await??)
    }

    async fn gauge<S: AsRef<str> + Sync + Send>(
        &self,
        stat: &str,
        val: &str,
        tags: &[S],
    ) -> Result<()> {
        let (reply, rx) = oneshot::channel();

        self.tx.send(Msg::Gauge {
            stat: stat.to_string(),
            val: val.to_string(),
            tags: tags.iter().map(|x| x.as_ref().to_string()).collect(),
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
