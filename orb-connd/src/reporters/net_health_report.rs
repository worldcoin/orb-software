use crate::network_manager::NetworkManager;
use crate::resolved::{HostnameResolution, LinkDnsStatus, Resolved};
use color_eyre::Result;
use std::time::{Duration, Instant};
use tokio::task::{self, JoinHandle};
use tracing::{error, info};

pub fn spawn(
    nm: NetworkManager,
    resolved: Resolved,
    rx: flume::Receiver<orb_connd_events::Connection>,
) -> JoinHandle<Result<()>> {
    info!("starting net_health_report");

    task::spawn(async move {
        while let Ok(conn_event) = rx.recv_async().await {
            if let Err(error) = report(&nm, &resolved, conn_event).await {
                error!(?error, "network health report failed: {error}");
            }
        }

        Ok(())
    })
}

async fn report(
    nm: &NetworkManager,
    resolved: &Resolved,
    primary_connection: orb_connd_events::Connection,
) -> Result<()> {
    let active_conns = nm.active_connections().await?;
    let connectivity_uri = nm.connectivity_check_uri().await?;
    let hostname = hostname_from_uri(&connectivity_uri).map(str::to_string);

    let mut report = NetHealthReport {
        primary_connection,
        connectivity_uri,
        hostname,
        connections: Vec::new(),
    };

    for conn in &active_conns {
        for iface in &conn.devices {
            let dns_status = resolved
                .link_status(iface)
                .await
                .map_err(|e| e.to_string());

            let dns_resolution = match &report.hostname {
                Some(hostname) => resolved
                    .resolve_hostname(iface, hostname)
                    .await
                    .map(Some)
                    .map_err(|e| e.to_string()),
                None => Ok(None),
            };

            let http_check: Result<_, String> = async {
                let client = reqwest::Client::builder()
                    .interface(iface)
                    .timeout(Duration::from_secs(5))
                    .build()?;

                let start = Instant::now();
                let res =
                    client.get(&report.connectivity_uri).send().await?;
                let elapsed = start.elapsed();

                Ok(HttpCheck::new(res, elapsed))
            }
            .await
            .map_err(|e: color_eyre::Report| e.to_string());

            report.connections.push(ConnectionReport {
                name: &conn.id,
                iface,
                dns_status,
                dns_resolution,
                http_check,
            });
        }
    }

    info!("network health report: {report:?}");

    Ok(())
}

#[derive(Debug)]
#[allow(dead_code)]
struct NetHealthReport<'a> {
    primary_connection: orb_connd_events::Connection,
    connectivity_uri: String,
    hostname: Option<String>,
    connections: Vec<ConnectionReport<'a>>,
}

#[derive(Debug)]
#[allow(dead_code)]
struct ConnectionReport<'a> {
    iface: &'a str,
    name: &'a str,
    dns_status: Result<LinkDnsStatus, String>,
    dns_resolution: Result<Option<HostnameResolution>, String>,
    http_check: Result<HttpCheck, String>,
}

#[derive(Debug)]
#[allow(dead_code)]
struct HttpCheck {
    status: reqwest::StatusCode,
    location: Option<String>,
    nm_status: Option<String>,
    content_length: Option<String>,
    elapsed: Duration,
}

impl HttpCheck {
    fn new(res: reqwest::Response, elapsed: Duration) -> Self {
        Self {
            status: res.status(),
            location: res
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string),
            nm_status: res
                .headers()
                .get("x-networkmanager-status")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string),
            content_length: res
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string),
            elapsed,
        }
    }
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
