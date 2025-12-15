use orb_connd_dbus::{ConndProxy, ConnectionState};
use std::time::Duration;
use tokio::{sync::watch, time};
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

pub fn spawn_watcher(
    connection: Connection,
    poll_interval: Duration,
    shutdown_token: CancellationToken,
) -> watch::Receiver<GlobalConnectivity> {
    let (tx, rx) = watch::channel(GlobalConnectivity::NotConnected);

    tokio::spawn(async move {
        let mut ticker = time::interval(poll_interval);
        ticker.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = shutdown_token.cancelled() => break,
                _ = ticker.tick() => {}
            }

            let proxy = match ConndProxy::new(&connection).await {
                Ok(p) => p,
                Err(e) => {
                    warn!("failed to create connd proxy: {e:?}");
                    continue;
                }
            };

            let state = match proxy.connection_state().await {
                Ok(s) => s,
                Err(e) => {
                    warn!("failed to get connd connection_state: {e:?}");
                    continue;
                }
            };

            let next = match state {
                ConnectionState::Connected => GlobalConnectivity::Connected,
                _ => GlobalConnectivity::NotConnected,
            };

            if *tx.borrow() != next {
                debug!("global connectivity changed: {state:?}");
            }
            let _ = tx.send(next);
        }
    });

    rx
}


