use crate::dbus::intf_impl::BackendStatusImpl;
use color_eyre::{eyre::eyre, Result};
use orb_connd_events::Connection;
use rkyv::AlignedVec;
use std::time::Duration;
use tokio::{sync::watch, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::debug;
use zenorb::zenoh;
use zenorb::Zenorb as ZSession;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobalConnectivity {
    Connected { ssid: Option<String> },
    NotConnected,
}

impl GlobalConnectivity {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }

    pub fn ssid(&self) -> Option<&str> {
        match self {
            Self::Connected { ssid } => ssid.as_deref(),
            Self::NotConnected => None,
        }
    }
}

pub struct ConnectivityWatcher {
    pub receiver: watch::Receiver<GlobalConnectivity>,
    pub task: JoinHandle<()>,
}

/// Spawn a connectivity watcher that subscribes to connd's zenoh topic for connection state.
pub async fn spawn_watcher(
    zsession: &ZSession,
    backend_status: BackendStatusImpl,
    shutdown_token: CancellationToken,
) -> Result<ConnectivityWatcher> {
    let (tx, rx) = watch::channel(GlobalConnectivity::NotConnected);

    let ctx = WatcherCtx { tx, backend_status };

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
    backend_status: BackendStatusImpl,
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
        orb_connd_events::ArchivedConnection::ConnectedGlobal(kind) => {
            let ssid = match kind {
                orb_connd_events::ArchivedConnectionKind::Wifi { ssid } => {
                    Some(ssid.to_string())
                }
                orb_connd_events::ArchivedConnectionKind::Ethernet
                | orb_connd_events::ArchivedConnectionKind::Cellular { .. } => None,
            };
            GlobalConnectivity::Connected { ssid }
        }
        _ => GlobalConnectivity::NotConnected,
    };

    let prev = ctx.tx.borrow().clone();
    if prev != connectivity {
        debug!("global connectivity changed: {connectivity:?}");

        // Check if SSID changed - if so, update snapshot and mark urgent
        let prev_ssid = prev.ssid();
        let new_ssid = connectivity.ssid();
        if prev_ssid != new_ssid {
            ctx.backend_status
                .update_active_ssid(new_ssid.map(String::from));
            ctx.backend_status.set_send_immediately();
        }
    }

    ctx.tx.send(connectivity).map_err(|e| eyre!("{e}"))?;

    Ok(())
}
