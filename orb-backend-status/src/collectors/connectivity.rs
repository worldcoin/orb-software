use orb_connd_dbus::{ConndProxy, ConnectionState};
use std::time::Duration;
use tokio::{sync::watch, task::JoinHandle, time};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};
use zbus::Connection;

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

/// Spawn a connectivity watcher that polls connd for connection state.
///
/// Performs an initial poll before spawning to ensure the state is
/// available immediately.
pub async fn spawn_watcher(
    connection: Connection,
    poll_interval: Duration,
    shutdown_token: CancellationToken,
) -> ConnectivityWatcher {
    // Poll once before spawning to get initial state
    let initial = poll_connectivity(&connection).await;
    let (tx, rx) = watch::channel(initial);

    let task = tokio::spawn(async move {
        let mut ticker = time::interval(poll_interval);
        ticker.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = shutdown_token.cancelled() => break,
                _ = ticker.tick() => {}
            }

            let next = poll_connectivity(&connection).await;
            if *tx.borrow() != next {
                debug!("global connectivity changed: {next:?}");
            }
            let _ = tx.send(next);
        }
    });

    ConnectivityWatcher { receiver: rx, task }
}

async fn poll_connectivity(connection: &Connection) -> GlobalConnectivity {
    let proxy = match ConndProxy::new(connection).await {
        Ok(p) => p,
        Err(e) => {
            warn!("failed to create connd proxy: {e:?}");
            return GlobalConnectivity::NotConnected;
        }
    };

    match proxy.connection_state().await {
        Ok(ConnectionState::Connected) => GlobalConnectivity::Connected,
        Ok(_) => GlobalConnectivity::NotConnected,
        Err(e) => {
            warn!("failed to get connd connection_state: {e:?}");
            GlobalConnectivity::NotConnected
        }
    }
}
