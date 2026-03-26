use crate::network_manager::{self, NetworkManager};
use crate::resolved::{HostnameResolution, LinkDnsStatus, Resolved};
use color_eyre::eyre::{bail, Context, ContextCompat};
use color_eyre::Result;
use futures::StreamExt;
use oes::NetworkInterface;
use serde::Serializer;
use speare::mini;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::fs;
use tracing::{info, warn};

pub struct Args {
    pub nm: NetworkManager,
    pub resolved: Resolved,
    pub zsender: zenorb::Sender,
    pub procfs: PathBuf,
    pub sysfs: PathBuf,
}

pub async fn report(ctx: mini::Ctx<Args>) -> Result<()> {
    info!("starting active connections reporter");

    async {
        let mut state_stream = ctx
            .nm
            .state_stream()
            .await
            .wrap_err("failed to subscribe to NetworkManager state stream")?;

        let mut primary_conn_stream =
            ctx.nm.primary_connection_stream().await.wrap_err(
                "faield to subscribe to NetworkManager primary connection stream",
            )?;

        let mut state = ctx.nm.state().await.wrap_err("failed to get nm state")?;
        let mut primary_conn = ctx
            .nm
            .primary_connection()
            .await
            .wrap_err("failed to get primary connection")?;

        let report = build_report(&primary_conn, &ctx)
            .await
            .wrap_err("building active connections report")?;

        publish_report(&ctx, report)
            .await
            .wrap_err("publishing active connections report")?;

        loop {
            tokio::select! {
                Some(_) = state_stream.next() => (),
                Some(_) = primary_conn_stream.next() => (),
            };

            let new_state = ctx.nm.state().await.wrap_err("failed to get nm state")?;

            let new_primary_conn = ctx
                .nm
                .primary_connection()
                .await
                .wrap_err("failed to get primary connection")?;

            let changed = (new_state != state) || (new_primary_conn != primary_conn);
            state = new_state;
            primary_conn = new_primary_conn;

            if changed {
                let report = build_report(&primary_conn, &ctx)
                    .await
                    .wrap_err("building active connections report")?;

                publish_report(&ctx, report)
                    .await
                    .wrap_err("publishing active connections report")?;
            }
        }

        #[allow(unreachable_code)]
        Ok(())
    }
    .await
    .inspect_err(|err| warn!("active connections report failed with: {err:?}"))
}

/// build report based on NM inputs and system inspection
async fn build_report(
    primary: &Option<network_manager::Connection>,
    ctx: &mini::Ctx<Args>,
) -> Result<ActiveConnections> {
    let active_conns = ctx.nm.active_connections().await?;
    let connectivity_uri = ctx.nm.connectivity_check_uri().await?;
    let hostname = hostname_from_uri(&connectivity_uri).map(str::to_string);

    let mut report = ActiveConnections {
        connectivity_uri,
        hostname,
        connections: Vec::new(),
        iface_routes: InterfaceRoutes::from_fs(&ctx.sysfs, &ctx.procfs).await?,
    };

    for conn in &active_conns {
        for iface in &conn.devices {
            let dns_status = ctx
                .resolved
                .link_status(iface)
                .await
                .map_err(|e| e.to_string());

            let dns_resolution = match &report.hostname {
                Some(hostname) => ctx
                    .resolved
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
                let res = client.get(&report.connectivity_uri).send().await?;
                let elapsed = start.elapsed();

                Ok(HttpCheck::new(res, elapsed))
            }
            .await
            .map_err(|e: color_eyre::Report| format!("{e:#}"));

            report.connections.push(Connection {
                primary: is_primary(primary, &conn.id),
                name: conn.id.clone(),
                iface: iface.to_owned(),
                has_internet: http_check.as_ref().is_ok_and(|x| x.status.is_success()),
                ipv4_addresses: conn.ipv4_addresses.clone(),
                ipv6_addresses: conn.ipv6_addresses.clone(),
                dns_status,
                dns_resolution,
                http_check,
            });
        }
    }

    Ok(report)
}

#[derive(Debug, serde::Serialize)]
struct ActiveConnections {
    connectivity_uri: String,
    hostname: Option<String>,
    connections: Vec<Connection>,
    iface_routes: Vec<InterfaceRoutes>,
}

#[derive(Debug, serde::Serialize)]
struct Connection {
    name: String,
    iface: String,
    primary: bool,
    has_internet: bool,
    ipv4_addresses: Vec<String>,
    ipv6_addresses: Vec<String>,
    dns_status: Result<LinkDnsStatus, String>,
    dns_resolution: Result<Option<HostnameResolution>, String>,
    http_check: Result<HttpCheck, String>,
}

#[derive(Debug, serde::Serialize)]
struct HttpCheck {
    #[serde(serialize_with = "serialize_status_code")]
    status: reqwest::StatusCode,
    location: Option<String>,
    nm_status: Option<String>,
    content_length: Option<String>,
    elapsed: Duration,
}

async fn publish_report(
    ctx: &mini::Ctx<Args>,
    report: ActiveConnections,
) -> Result<()> {
    info!("{report:#?}");

    let report: oes::ActiveConnections = report.try_into()?;

    let _ = ctx.publish("active_connections", report.clone());

    let bytes = serde_json::to_vec(&report)?;
    ctx.zsender
        .publisher("oes/active_connections")?
        .put(&bytes)
        .await
        .map_err(|e| color_eyre::eyre::eyre!("{e}"))?;

    Ok(())
}

fn serialize_status_code<S: Serializer>(
    status: &reqwest::StatusCode,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_u16(status.as_u16())
}

fn is_primary(primary: &Option<network_manager::Connection>, conn_name: &str) -> bool {
    let Some(primary) = primary else { return false };

    match primary {
        network_manager::Connection::Cellular { .. } => {
            conn_name.to_lowercase().contains("cellular")
        }

        network_manager::Connection::Ethernet => {
            let conn_name = conn_name.to_lowercase();
            conn_name.contains("wired") || conn_name.contains("ethernet")
        }

        network_manager::Connection::Wifi { ssid } => ssid == conn_name,
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

impl TryFrom<ActiveConnections> for oes::ActiveConnections {
    type Error = color_eyre::Report;

    fn try_from(val: ActiveConnections) -> Result<Self> {
        let connections = val
            .connections
            .into_iter()
            .map(|c| {
                let iface = match c.iface.to_lowercase().get(..3) {
                    Some("eth") => NetworkInterface::Ethernet,
                    Some("wla") => NetworkInterface::WiFi,
                    Some("wwa") => NetworkInterface::Cellular,
                    _ => bail!("{} is not a valid network interface", c.iface),
                };

                Ok(oes::Connection {
                    name: c.name,
                    iface,
                    primary: c.primary,
                    has_internet: c.has_internet,
                })
            })
            .collect::<Result<_>>()?;

        Ok(oes::ActiveConnections {
            connectivity_uri: val.connectivity_uri,
            connections,
        })
    }
}

#[derive(Debug, serde::Serialize)]
struct InterfaceRoutes {
    ifname: String,
    operstate: String,
    routes: Vec<Route>,
}

#[derive(Debug, serde::Serialize)]
struct Route {
    destination: String,
    metric: u64,
}

type Iface = String;
type Operstate = String;

impl InterfaceRoutes {
    async fn from_fs(
        sysfs: impl AsRef<Path>,
        procfs: impl AsRef<Path>,
    ) -> Result<Vec<InterfaceRoutes>> {
        let mut ifaces = get_interfaces_operstate(sysfs).await?;
        let routes = get_routes(procfs).await?;

        let mut ifaceroutes: HashMap<String, (Operstate, Vec<Route>)> =
            HashMap::with_capacity(ifaces.len());

        for (iface, route) in routes.into_iter() {
            match ifaceroutes.entry(iface) {
                Entry::Occupied(mut entry) => {
                    entry.get_mut().1.push(route);
                }

                Entry::Vacant(entry) => {
                    let Some(operstate) = ifaces.remove(entry.key()) else {
                        continue;
                    };

                    entry.insert((operstate, vec![route]));
                }
            }
        }

        Ok(ifaceroutes
            .into_iter()
            .map(|(ifname, (operstate, routes))| InterfaceRoutes {
                ifname,
                operstate,
                routes,
            })
            .collect())
    }
}

async fn get_interfaces_operstate(
    sysfs: impl AsRef<Path>,
) -> Result<HashMap<Iface, Operstate>> {
    let ifaces_dir = sysfs.as_ref().join("class").join("net");
    let mut dir = fs::read_dir(ifaces_dir).await?;
    let mut interfaces = HashMap::new();

    while let Some(entry) = dir.next_entry().await? {
        let path = entry.path();
        let Some(iface) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        let operstate = fs::read_to_string(path.join("operstate")).await?;
        interfaces.insert(iface.to_string(), operstate.trim().to_string());
    }

    Ok(interfaces)
}

async fn get_routes(procfs: impl AsRef<Path>) -> Result<Vec<(Iface, Route)>> {
    let path = procfs.as_ref().join("net").join("route");
    let routes = fs::read_to_string(path).await?;

    routes
        .lines()
        .skip(1) // header, see test for context
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut cols = line.split_whitespace();

            let iface = cols
                .next()
                .wrap_err_with(|| format!("invalid /proc/net/route line: {line}"))?;

            let destination = cols
                .next()
                .wrap_err_with(|| format!("invalid /proc/net/route line: {line}"))?;

            cols.next(); // gateway
            cols.next(); // flags
            cols.next(); // refcnt
            cols.next(); // use

            let metric = cols
                .next()
                .wrap_err_with(|| format!("invalid /proc/net/route line: {line}"))?;

            Ok((
                iface.to_owned(),
                Route {
                    destination: destination.to_owned(),
                    metric: metric.parse()?,
                },
            ))
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;
    use async_tempfile::TempDir;

    #[tokio::test]
    async fn get_interfaces_reads_operstate_for_each_interface() {
        // Arrange
        let sysfs = TempDir::new().await.unwrap();
        let sysfs_path = sysfs.to_path_buf();
        let net_dir = sysfs_path.join("class").join("net");

        fs::create_dir_all(net_dir.join("eth0")).await.unwrap();
        fs::create_dir_all(net_dir.join("wlan0")).await.unwrap();
        fs::create_dir_all(net_dir.join("wwan0")).await.unwrap();

        fs::write(net_dir.join("eth0").join("operstate"), "down\n")
            .await
            .unwrap();
        fs::write(net_dir.join("wlan0").join("operstate"), "up\n")
            .await
            .unwrap();
        fs::write(net_dir.join("wwan0").join("operstate"), "unknown\n")
            .await
            .unwrap();

        // Act
        let interfaces = get_interfaces_operstate(&sysfs_path).await.unwrap();

        // Assert
        assert_eq!(
            interfaces,
            HashMap::from([
                ("eth0".to_string(), "down".to_string()),
                ("wlan0".to_string(), "up".to_string()),
                ("wwan0".to_string(), "unknown".to_string()),
            ])
        );
    }

    #[tokio::test]
    async fn get_routes_reads_routes_from_procfs() {
        // Arrange
        let procfs = TempDir::new().await.unwrap();
        let procfs_path = procfs.to_path_buf();
        let route_dir = procfs_path.join("net");
        let route_path = route_dir.join("route");

        fs::create_dir_all(&route_dir).await.unwrap();
        fs::write(
            &route_path,
            concat!(
                "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n",
                "eth0\t0010A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n",
                "wlan0\t00000000\t01006C0A\t0003\t0\t0\t400\t00000000\t0\t0\t0\n",
                "wwan0\t00000000\t39A54664\t0003\t0\t0\t500\t00000000\t0\t0\t0\n",
                "wlan0\t00006C0A\t00000000\t0001\t0\t0\t400\t0000FFFF\t0\t0\t0\n",
                "wwan0\t30A54664\t00000000\t0001\t0\t0\t500\tF0FFFFFF\t0\t0\t0\n",
            ),
        )
        .await
        .unwrap();

        // Act
        let routes = get_routes(&procfs_path).await.unwrap();

        // Assert
        assert_eq!(routes.len(), 5);
        assert_eq!(routes[0].0, "eth0");
        assert_eq!(routes[0].1.destination, "0010A8C0");
        assert_eq!(routes[0].1.metric, 100);
        assert_eq!(routes[1].0, "wlan0");
        assert_eq!(routes[1].1.destination, "00000000");
        assert_eq!(routes[1].1.metric, 400);
        assert_eq!(routes[2].0, "wwan0");
        assert_eq!(routes[2].1.destination, "00000000");
        assert_eq!(routes[2].1.metric, 500);
        assert_eq!(routes[3].0, "wlan0");
        assert_eq!(routes[3].1.destination, "00006C0A");
        assert_eq!(routes[3].1.metric, 400);
        assert_eq!(routes[4].0, "wwan0");
        assert_eq!(routes[4].1.destination, "30A54664");
        assert_eq!(routes[4].1.metric, 500);
    }
}
