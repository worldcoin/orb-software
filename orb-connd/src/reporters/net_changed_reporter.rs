use crate::network_manager::{Connection, NetworkManager};
use crate::resolved::Resolved;
use color_eyre::{eyre::eyre, Result};
use futures::StreamExt;
use orb_connd_events::ConnectionKind;
use rusty_network_manager::dbus_interface_types::NMState;
use std::fmt::Write;
use std::time::{Duration, Instant};
use tokio::{
    task::{self, JoinHandle},
    time,
};
use tracing::{error, info, warn};

static BACKOFF: Duration = Duration::from_secs(5);

pub fn spawn(
    nm: NetworkManager,
    resolved: Resolved,
    zsender: zenorb::Sender,
) -> JoinHandle<Result<()>> {
    info!("starting net_changed reporter");

    task::spawn(async move {
        loop {
            if let Err(e) = report_loop(&nm, &resolved, &zsender).await {
                error!(error = ?e, "net changed loop error, retrying in {}s. error: {e}", BACKOFF.as_secs());
            }

            time::sleep(BACKOFF).await;
        }
    })
}

async fn report_loop(
    nm: &NetworkManager,
    resolved: &Resolved,
    zsender: &zenorb::Sender,
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

    if is_connected(&conn_event) {
        report(nm, resolved, &conn_event).await?;
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

            if is_connected(&conn_event) {
                report(nm, resolved, &conn_event).await?;
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

async fn report(
    nm: &NetworkManager,
    resolved: &Resolved,
    conn_event: &orb_connd_events::Connection,
) -> Result<()> {
    let active_conns = nm.active_connections().await?;
    let connectivity_uri = nm.connectivity_check_uri().await?;
    let hostname = hostname_from_uri(&connectivity_uri);

    let mut msg = String::new();
    writeln!(msg, "network report:")?;
    writeln!(msg, "  primary connection: {conn_event:?}")?;

    for conn in &active_conns {
        writeln!(msg, "  [{}]: {conn:?}", conn.id)?;

        for iface in &conn.devices {
            match resolved.link_status(iface).await {
                Ok(status) => {
                    writeln!(msg, "    [{iface}] resolvectl status: {status:?}")?
                }

                Err(e) => {
                    warn!(iface, error = ?e, "[{iface}] resolvectl status failed")
                }
            }

            let Some(hostname) = hostname else { continue };
            match resolved.resolve_hostname(iface, hostname).await {
                Ok(resolution) => writeln!(
                    msg,
                    "    [{iface}] resolvectl query {hostname}: {resolution:?}"
                )?,

                Err(e) => {
                    warn!(iface, host = hostname, error = ?e, "[{iface}] resolvectl query {hostname} failed")
                }
            }
        }
    }

    match connectivity_check(&connectivity_uri).await {
        Ok(check) => {
            writeln!(msg, "  connectivity check GET ok {connectivity_uri}:")?;
            writeln!(msg, "    status: {}", check.status)?;
            if let Some(loc) = &check.location {
                writeln!(msg, "    Location: {loc}")?;
            }
            if let Some(nms) = &check.nm_status {
                writeln!(msg, "    X-NetworkManager-Status: {nms}")?;
            }
            if let Some(cl) = &check.content_length {
                writeln!(msg, "    Content-Length: {cl}")?;
            }
            writeln!(msg, "    elapsed: {}ms", check.elapsed.as_millis())?;
        }

        Err(e) => {
            warn!(
                uri = connectivity_uri,
                error = ?e,
                "connectivity check GET failed {connectivity_uri}"
            );
        }
    }

    info!("{msg}");

    Ok(())
}

#[derive(Debug)]
struct ConnectivityCheck {
    status: reqwest::StatusCode,
    location: Option<String>,
    nm_status: Option<String>,
    content_length: Option<String>,
    elapsed: Duration,
}

async fn connectivity_check(uri: &str) -> Result<ConnectivityCheck> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()?;
    let start = Instant::now();
    let resp = client.get(uri).send().await?;
    let elapsed = start.elapsed();

    let status = resp.status();
    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let nm_status = resp
        .headers()
        .get("x-networkmanager-status")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let content_length = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    Ok(ConnectivityCheck {
        status,
        location,
        nm_status,
        content_length,
        elapsed,
    })
}

fn hostname_from_uri(uri: &str) -> Option<&str> {
    let after_scheme = uri.split("://").nth(1)?;
    let host_and_rest = after_scheme.split('/').next()?;
    let host = host_and_rest.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}
