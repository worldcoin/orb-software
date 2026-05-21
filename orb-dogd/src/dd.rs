use dogstatsd::{Client, DogstatsdError, Options};
use flume::{Sender, TrySendError};
use std::thread;
use tracing::error;

use super::{Metric, MetricEmitter, MetricError};

pub struct DogstatsdClient {
    tx: Sender<Metric>,
}

impl DogstatsdClient {
    /// Bind the UDP socket and spawn the consumer thread,
    /// propagating metrics to the metric collector
    ///
    /// Fails if [`Client::new`] fails (socket bind / permission).
    /// The worker runs until all [`Sender`]s are dropped.
    pub fn new() -> Result<Self, DogstatsdError> {
        let (tx, rx) = flume::bounded(1024);

        let datadog_options = Options::default();
        let client = Client::new(datadog_options)?;

        thread::spawn(move || {
            while let Ok(metric) = rx.recv() {
                match metric {
                    Metric::Count { stat, val, tags } => {
                        if let Err(e) = client.count(stat, val, tags) {
                            error!("emitting metric failed with: {e}");
                        }
                    }
                    Metric::Gauge { stat, val, tags } => {
                        if let Err(e) = client.gauge(stat, val.to_string(), tags) {
                            error!("emitting metric failed with: {e}");
                        }
                    }
                    Metric::Histogram { stat, val, tags } => {
                        if let Err(e) = client.histogram(stat, val.to_string(), tags) {
                            error!("emitting metric failed with: {e}");
                        }
                    }
                    Metric::Distribution { stat, val, tags } => {
                        if let Err(e) = client.distribution(stat, val.to_string(), tags) {
                            error!("emitting metric failed with: {e}");
                        }
                    }
                }
            }
        });

        Ok(Self { tx })
    }
}

impl MetricEmitter for DogstatsdClient {
    fn emit(&self, metric: Metric) -> Result<(), MetricError> {
        match self.tx.try_send(metric) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(e)) => Err(MetricError::ChannelFull(e)),
            Err(TrySendError::Disconnected(e)) => Err(MetricError::ChannelClosed(e)),
        }
    }
}
