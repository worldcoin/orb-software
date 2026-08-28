use crate::status_client::{self, OesStatusApiV2, StatusClient};
use crate::stream::Event;
use orb_dogd::MetricEmitter;
use std::time::Duration;
use tracing::{debug, error, warn};

const RECV_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_BATCH_EVENTS: usize = 100;

pub fn run_oes_flush_loop<M: MetricEmitter + Clone>(
    oes_rx: flume::Receiver<Event>,
    client: StatusClient<M>,
    shutdown_rx: flume::Receiver<()>,
) {
    let mut buffer: Vec<Event> = Vec::new();

    loop {
        match oes_rx.recv_timeout(RECV_TIMEOUT) {
            Ok(event) => {
                buffer.push(event);
                drain_available(&oes_rx, &mut buffer);
            }

            Err(flume::RecvTimeoutError::Timeout) => {}

            Err(flume::RecvTimeoutError::Disconnected) => {
                debug!("OES channel closed, exiting flush loop");
                break;
            }
        }

        maybe_flush(&client, &mut buffer);

        if matches!(
            shutdown_rx.try_recv(),
            Ok(()) | Err(flume::TryRecvError::Disconnected)
        ) {
            if !buffer.is_empty() {
                debug!(count = buffer.len(), "Shutdown: attempting final OES flush");

                if let Err(e) = flush_events(&client, &buffer) {
                    warn!("Final OES flush failed: {e}");
                }
            }

            break;
        }
    }
}

fn drain_available(rx: &flume::Receiver<Event>, buffer: &mut Vec<Event>) {
    while let Ok(event) = rx.try_recv() {
        buffer.push(event);
    }
}

fn maybe_flush<M: MetricEmitter + Clone>(
    client: &StatusClient<M>,
    buffer: &mut Vec<Event>,
) {
    if buffer.is_empty() {
        return;
    }

    let batch_size = buffer.len().min(MAX_BATCH_EVENTS);
    let batch = &buffer[..batch_size];

    match flush_events(client, batch) {
        Ok(true) => {
            debug!(count = batch_size, "OES flush successful");
            buffer.drain(..batch_size);
        }

        Ok(false) => {}

        Err(e) => {
            error!(
                count = buffer.len(),
                "OES flush failed, events remain buffered: {e}",
            );
        }
    }
}

/// Returns `Ok(true)` if the batch was sent (caller should drop it from the
/// buffer), `Ok(false)` if it couldn't be sent yet for a reason expected to
/// resolve itself (no attest token yet — keep it buffered), or `Err` on a
/// genuine failure.
fn flush_events<M: MetricEmitter + Clone>(
    client: &StatusClient<M>,
    events: &[Event],
) -> eyre::Result<bool> {
    let req = OesStatusApiV2 {
        oes: Some(events.to_vec()),
        ..Default::default()
    };

    match client.req(req) {
        Err(status_client::Err::MissingAttestToken) => Ok(false),
        Err(status_client::Err::Other(e)) => Err(e),
        Ok(_) => Ok(true),
    }
}
