use crate::network_manager::NetworkManager;
use crate::resolved::Resolved;
use color_eyre::Result;
use std::fmt::Write;
use std::time::{Duration, Instant};
use tokio::task::{self, JoinHandle};
use tracing::{error, info, warn};

pub fn spawn(
    nm: NetworkManager,
    resolved: Resolved,
    rx: flume::Receiver<orb_connd_events::Connection>,
) -> JoinHandle<Result<()>> {
    info!("starting net_health_report");

    task::spawn(async move {
        while let Ok(conn_event) = rx.recv_async().await {
            report(&nm, &resolved, &conn_event).await;
        }

        Ok(())
    })
}

async fn report(
    nm: &NetworkManager,
    resolved: &Resolved,
    conn_event: &orb_connd_events::Connection,
) {
    let active_conns = match nm.active_connections().await {
        Ok(conns) => conns,
        Err(e) => {
            error!(error = ?e, "network report failed: {e}");
            return;
        }
    };
    let connectivity_uri = match nm.connectivity_check_uri().await {
        Ok(uri) => uri,
        Err(e) => {
            error!(error = ?e, "network report failed: {e}");
            return;
        }
    };
    let hostname = hostname_from_uri(&connectivity_uri);

    let mut msg = String::new();
    let _ = writeln!(msg, "network report:");
    let _ = writeln!(msg, "  primary connection: {conn_event:?}");

    for conn in &active_conns {
        let _ = writeln!(msg, "  [{}]: {conn:?}", conn.id);

        for iface in &conn.devices {
            match resolved.link_status(iface).await {
                Ok(status) => {
                    let _ =
                        writeln!(msg, "    [{iface}] resolvectl status: {status:?}");
                }

                Err(e) => {
                    warn!(iface, error = ?e, "[{iface}] resolvectl status failed")
                }
            }

            let Some(hostname) = hostname else { continue };
            match resolved.resolve_hostname(iface, hostname).await {
                Ok(resolution) => {
                    let _ = writeln!(
                        msg,
                        "    [{iface}] resolvectl query {hostname}: {resolution:?}"
                    );
                }

                Err(e) => {
                    warn!(iface, host = hostname, error = ?e, "[{iface}] resolvectl query {hostname} failed")
                }
            }

            match connectivity_check(iface, &connectivity_uri).await {
                Ok(check) => {
                    let result = if check.status.is_success() {
                        "ok"
                    } else {
                        "fail"
                    };
                    let _ = writeln!(msg, "    [{iface}] connectivity check GET {result} {connectivity_uri}:");
                    let _ = writeln!(msg, "      status: {}", check.status);
                    if let Some(loc) = &check.location {
                        let _ = writeln!(msg, "      Location: {loc}");
                    }
                    if let Some(nms) = &check.nm_status {
                        let _ = writeln!(msg, "      X-NetworkManager-Status: {nms}");
                    }
                    if let Some(cl) = &check.content_length {
                        let _ = writeln!(msg, "      Content-Length: {cl}");
                    }
                    let _ =
                        writeln!(msg, "      elapsed: {}ms", check.elapsed.as_millis());
                }

                Err(e) => {
                    warn!(
                        iface,
                        uri = connectivity_uri,
                        error = ?e,
                        "[{iface}] connectivity check GET timeout {connectivity_uri}"
                    );
                }
            }
        }
    }

    info!("{msg}");
}

#[derive(Debug)]
struct ConnectivityCheck {
    status: reqwest::StatusCode,
    location: Option<String>,
    nm_status: Option<String>,
    content_length: Option<String>,
    elapsed: Duration,
}

async fn connectivity_check(iface: &str, uri: &str) -> Result<ConnectivityCheck> {
    let client = reqwest::Client::builder()
        .interface(iface)
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
