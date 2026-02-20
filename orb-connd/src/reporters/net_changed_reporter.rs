use crate::network_manager::{Connection, NetworkManager};
use color_eyre::{eyre::eyre, Result};
use futures::StreamExt;
use orb_connd_events::ConnectionKind;
use rusty_network_manager::dbus_interface_types::NMState;
use std::time::Duration;
use tokio::{
    task::{self, JoinHandle},
    time,
};
use tracing::{error, info, warn};

static BACKOFF: Duration = Duration::from_secs(5);

pub fn spawn(
    nm: NetworkManager,
    zsender: zenorb::Sender,
    health_tx: flume::Sender<orb_connd_events::Connection>,
) -> JoinHandle<Result<()>> {
    info!("starting net_changed reporter");

    task::spawn(async move {
        loop {
            if let Err(e) =
                report_loop(&nm, &zsender, &health_tx).await
            {
                error!(error = ?e, "net changed loop error, retrying in {}s. error: {e}", BACKOFF.as_secs());
            }

            time::sleep(BACKOFF).await;
        }
    })
}

async fn report_loop(
    nm: &NetworkManager,
    zsender: &zenorb::Sender,
    health_tx: &flume::Sender<orb_connd_events::Connection>,
) -> Result<()> {
    let publisher = zsender.publisher("net/changed")?;
    let mut state_stream = nm.state_stream().await?;
    let mut primary_conn_stream = nm.primary_connection_stream().await?;

    let nm_state = nm.state().await?;
    let mut conn_event = connection_event(nm_state, nm.primary_connection().await?);

    let bytes = rkyv::to_bytes::<_, 64>(&conn_event)?;
    publisher
        .put(bytes.into_vec())
        .await
        .map_err(|e| eyre!("{e}"))?;

    if is_connected(&conn_event)
        && let Err(e) = health_tx.send(conn_event.clone())
    {
        warn!(error = ?e, "failed to send health report event");
    }

    loop {
        tokio::select! {
            _ = state_stream.next() => {}
            _ = primary_conn_stream.next() => {}
        };

        let new_conn_event =
            connection_event(nm.state().await?, nm.primary_connection().await?);

        let changed = conn_event != new_conn_event;
        conn_event = new_conn_event;

        if changed {
            let bytes = rkyv::to_bytes::<_, 64>(&conn_event)?;
            publisher
                .put(bytes.into_vec())
                .await
                .map_err(|e| eyre!("{e}"))?;

            if is_connected(&conn_event)
                && let Err(e) = health_tx.send(conn_event.clone())
            {
                warn!(error = ?e, "failed to send health report event");
            }
        }
    }
}

fn connection_event(
    state: NMState,
    active_conn: Option<Connection>,
) -> orb_connd_events::Connection {
    use orb_connd_events::Connection::*;
    let kind = active_conn.map(|c| match c {
        Connection::Cellular { apn } => ConnectionKind::Cellular { apn },
        Connection::Wifi { ssid } => ConnectionKind::Wifi { ssid },
        Connection::Ethernet => ConnectionKind::Ethernet,
    });

    match (state, kind) {
        (NMState::CONNECTED_GLOBAL, Some(kind)) => ConnectedGlobal(kind),
        (NMState::CONNECTED_SITE, Some(kind)) => ConnectedSite(kind),
        (NMState::CONNECTED_LOCAL, Some(kind)) => ConnectedLocal(kind),
        (NMState::CONNECTING, _) => Connecting,
        (NMState::DISCONNECTING, _) => Disconnecting,
        (NMState::UNKNOWN | NMState::ASLEEP | NMState::DISCONNECTED, _) => Disconnected,
        _ => Disconnected,
    }
}

fn is_connected(conn_event: &orb_connd_events::Connection) -> bool {
    matches!(
        conn_event,
        orb_connd_events::Connection::ConnectedGlobal(_)
            | orb_connd_events::Connection::ConnectedSite(_)
            | orb_connd_events::Connection::ConnectedLocal(_)
    )
}
