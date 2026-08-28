use color_eyre::eyre::eyre;
use color_eyre::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tracing::warn;

use crate::status_client::StatusClient;
use orb_dogd::MetricEmitter;
use std::thread::JoinHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub name: String,
    pub created_at: i64,
    pub payload: Option<serde_json::Value>,
}

pub struct Payload {
    pub headers: oes::Headers,
    pub event: Event,
}

#[derive(Debug)]
struct Throttle {
    value: Duration,
    last_publish: Option<Instant>,
}

#[derive(Debug, Clone)]
pub struct OrbEventStream {
    tx: flume::Sender<Event>,
    cache: Arc<Mutex<HashMap<String, Event>>>,
    throttle: Arc<Mutex<HashMap<String, Throttle>>>,
}

impl OrbEventStream {
    pub fn start<M: MetricEmitter + Clone + Send + 'static>(
        status_client: StatusClient<M>,
        shutdown_rx: flume::Receiver<()>,
    ) -> (Self, JoinHandle<()>) {
        let (tx, rx) = flume::unbounded();

        let handle = std::thread::spawn(move || {
            crate::flusher::run_oes_flush_loop(rx, status_client, shutdown_rx);
        });

        (
            Self {
                tx,
                cache: Default::default(),
                throttle: Default::default(),
            },
            handle,
        )
    }

    /// Returns a clone of all currently cached OES events
    pub fn cached(&self) -> Result<Vec<Event>> {
        let values = self
            .cache
            .lock()
            .map_err(|_| eyre!("cache lock poison"))?
            .values()
            .cloned()
            .collect();

        Ok(values)
    }

    pub fn throttle(&self, throttles: &[(&str, Duration)]) -> Result<()> {
        let mut throttle_map = self
            .throttle
            .lock()
            .map_err(|_| eyre!("throttle lock poison"))?;

        for (evt_name, throttle) in throttles {
            throttle_map.insert(
                evt_name.to_string(),
                Throttle {
                    value: *throttle,
                    last_publish: None,
                },
            );
        }

        Ok(())
    }

    pub fn ingest(&self, payload: Payload) -> Result<()> {
        match payload.headers.mode {
            oes::Mode::CacheOnly => {
                let mut cache =
                    self.cache.lock().map_err(|_| eyre!("cache lock poison"))?;
                cache.insert(payload.event.name.clone(), payload.event);
            }

            oes::Mode::Sticky => {
                let mut cache =
                    self.cache.lock().map_err(|_| eyre!("cache lock poison"))?;
                cache.insert(payload.event.name.clone(), payload.event.clone());

                if self.is_throttled(&payload.event.name)? {
                    return Ok(());
                }

                let _ = self.tx.send(payload.event).inspect_err(|e| {
                    warn!("Failed to send OES event over channel: {e}")
                });
            }

            oes::Mode::Normal => {
                if self.is_throttled(&payload.event.name)? {
                    return Ok(());
                }

                let _ = self.tx.send(payload.event).inspect_err(|e| {
                    warn!("Failed to send OES event over channel: {e}")
                });
            }
        }

        Ok(())
    }

    fn is_throttled(&self, evt_name: &str) -> Result<bool> {
        let can_send = match self
            .throttle
            .lock()
            .map_err(|_| eyre!("throttle lock poison"))?
            .get_mut(evt_name)
        {
            None => false,

            Some(throttle) => {
                if throttle
                    .last_publish
                    .is_some_and(|lp| lp.elapsed() < throttle.value)
                {
                    true
                } else {
                    throttle.last_publish = Some(Instant::now());
                    false
                }
            }
        };

        Ok(can_send)
    }
}
