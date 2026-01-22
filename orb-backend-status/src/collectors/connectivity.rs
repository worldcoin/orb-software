use color_eyre::{eyre::eyre, Result};
use orb_connd_events::Connection;
use rkyv::AlignedVec;
use std::time::Duration;
use tokio::{sync::watch, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalConnectivity {
    Connected,
    NotConnected,
}

impl GlobalConnectivity {
    pub fn is_connected(self) -> bool {
        matches!(self, Self::Connected)
    }
}

pub struct ConnectivityWatcher {
    pub receiver: watch::Receiver<GlobalConnectivity>,
    pub task: JoinHandle<()>,
}

/// Spawn a connectivity watcher that subscribes to connd's zenoh topic for connection state.
pub async fn spawn_watcher(
    zsession: &zenorb::Session,
    shutdown_token: CancellationToken,
) -> Result<ConnectivityWatcher> {
    let (tx, rx) = watch::channel(GlobalConnectivity::NotConnected);

    let ctx = WatcherCtx { tx };

    let mut tasks = zsession
        .receiver(ctx)
        .querying_subscriber(
            "connd/net/changed",
            Duration::from_millis(15),
            handle_connection_event,
        )
        .run()
        .await?;

    let subscriber_task = tasks
        .pop()
        .ok_or_else(|| eyre!("expected subscriber task"))?;

    let task = tokio::spawn(async move {
        shutdown_token.cancelled().await;
        subscriber_task.abort();
    });

    Ok(ConnectivityWatcher { receiver: rx, task })
}

#[derive(Clone)]
struct WatcherCtx {
    tx: watch::Sender<GlobalConnectivity>,
}

async fn handle_connection_event(
    ctx: WatcherCtx,
    sample: zenoh::sample::Sample,
) -> Result<()> {
    let payload = sample.payload().to_bytes();
    let mut bytes = AlignedVec::with_capacity(payload.len());
    bytes.extend_from_slice(&payload);

    let archived =
        rkyv::check_archived_root::<Connection>(&bytes).map_err(|e| eyre!("{e}"))?;

    let connectivity = match archived {
        orb_connd_events::ArchivedConnection::ConnectedGlobal(_) => {
            GlobalConnectivity::Connected
        }
        _ => GlobalConnectivity::NotConnected,
    };

    let prev = *ctx.tx.borrow();
    if prev != connectivity {
        debug!("global connectivity changed: {connectivity:?}");
    }

    ctx.tx.send(connectivity).map_err(|e| eyre!("{e}"))?;

    Ok(())
}
