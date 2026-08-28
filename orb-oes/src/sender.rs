use crate::status_client::{self, OesStatusApiV2, StatusClient};
use crate::stream::OrbEventStream;
use orb_dogd::MetricEmitter;
use std::time::Duration;
use tracing::error;

pub fn run_cached_snapshot_loop<M: MetricEmitter + Clone>(
    client: StatusClient<M>,
    oes: OrbEventStream,
    interval: Duration,
    shutdown_rx: flume::Receiver<()>,
) {
    while let Err(flume::RecvTimeoutError::Timeout) = shutdown_rx.recv_timeout(interval)
    {
        if let Err(e) = send_cached_snapshot(&client, &oes) {
            error!("failed to send cached OES snapshot: {e:?}");
        }
    }
}

fn send_cached_snapshot<M: MetricEmitter + Clone>(
    client: &StatusClient<M>,
    oes: &OrbEventStream,
) -> eyre::Result<()> {
    let req = OesStatusApiV2 {
        oes: Some(oes.cached()?),
        oes_cached: true,
        ..Default::default()
    };

    match client.req(req) {
        Err(status_client::Err::MissingAttestToken) => Ok(()),
        Err(status_client::Err::Other(e)) => Err(e),
        Ok(_) => Ok(()),
    }
}
