use crate::network_manager::NetworkManager;
use crate::resolved::{HostnameResolution, LinkDnsStatus, Resolved};
use color_eyre::eyre::{bail, eyre, Context, ContextCompat, OptionExt};
use color_eyre::Result;
use futures::StreamExt;
use oes::NetworkInterface;
use serde::Serializer;
use speare::mini;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::fs;
use tracing::{error, info};

pub struct Args {
    pub nm: NetworkManager,
    pub resolved: Resolved,
    pub zsender: zenorb::Sender,
}

pub async fn report(ctx: mini::Ctx<Args>) -> Result<()> {
    info!("starting active connections reporter");
    let mut state_stream = ctx
        .nm
        .state_stream()
        .await
        .wrap_err("failed to subscribe to NetworkManager state stream")?;

    let mut primary_conn_stream =
        ctx.nm.primary_connection_stream().await.wrap_err(
            "faield to subscribe to NetworkManager primary connection stream",
        )?;

    loop {
        tokio::select! {
            Some(_) = state_stream.next() => {}
            Some(_) = primary_conn_stream.next() => {}
        };

        let _ = build_and_send_report(&ctx.nm, &ctx.resolved, &ctx.zsender)
            .await
            .inspect_err(|error| {
                error!(?error, "active connections report failed: {error}")
            });
    }
}

// build report based on NM inputs and system inspection
//

async fn build_and_send_report(
    nm: &NetworkManager,
    resolved: &Resolved,
    zsender: &zenorb::Sender,
) -> Result<()> {
    let active_conns = nm.active_connections().await?;
    let connectivity_uri = nm.connectivity_check_uri().await?;
    let hostname = hostname_from_uri(&connectivity_uri).map(str::to_string);

    let mut report = ActiveConnections {
        connectivity_uri,
        hostname,
        connections: Vec::new(),
    };

    for conn in &active_conns {
        for iface in &conn.devices {
            let dns_status =
                resolved.link_status(iface).await.map_err(|e| e.to_string());

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
                let res = client.get(&report.connectivity_uri).send().await?;
                let elapsed = start.elapsed();

                Ok(HttpCheck::new(res, elapsed))
            }
            .await
            .map_err(|e: color_eyre::Report| format!("{e:#}"));

            report.connections.push(Connection {
                primary: is_primary(&conn.id),
                name: &conn.id,
                iface,
                has_internet: http_check.as_ref().is_ok_and(|x| x.status.is_success()),
                ipv4_addresses: &conn.ipv4_addresses,
                ipv6_addresses: &conn.ipv6_addresses,
                dns_status,
                dns_resolution,
                http_check,
            });
        }
    }

    info!("{report:#?}");

    if let Err(e) = publish_report(report.try_into()?, zsender).await {
        error!("failed to publish active connections report: {e}");
    }

    Ok(())
}

#[derive(Debug, serde::Serialize)]
struct ActiveConnections<'a> {
    connectivity_uri: String,
    hostname: Option<String>,
    connections: Vec<Connection<'a>>,
}

#[derive(Debug, serde::Serialize)]
struct Connection<'a> {
    name: &'a str,
    iface: &'a str,
    primary: bool,
    has_internet: bool,
    ipv4_addresses: &'a [String],
    ipv6_addresses: &'a [String],
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
    report: oes::ActiveConnections,
    zsender: &zenorb::Sender,
) -> Result<()> {
    let bytes = serde_json::to_vec(&report)?;
    zsender
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

fn is_primary(conn_name: &str) -> bool {
    todo!()
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

impl<'a> TryFrom<ActiveConnections<'a>> for oes::ActiveConnections {
    type Error = color_eyre::Report;

    fn try_from(val: ActiveConnections<'a>) -> Result<Self> {
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
                    name: c.name.into(),
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

#[derive(Debug)]
struct InterfaceRoutes {
    ifname: String,
    operstate: String,
    routes: Vec<Route>,
}

#[derive(Debug)]
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

        fs::write(net_dir.join("eth0").join("operstate"), "up\n")
            .await
            .unwrap();
        fs::write(net_dir.join("wlan0").join("operstate"), "unknown\n")
            .await
            .unwrap();
        fs::write(net_dir.join("wwan0").join("operstate"), "down\n")
            .await
            .unwrap();

        // Act
        let interfaces = get_interfaces_operstate(&sysfs_path).await.unwrap();

        // Assert
        assert_eq!(
            interfaces,
            vec![
                ("eth0".to_string(), "up".to_string()),
                ("wlan0".to_string(), "unknown".to_string()),
                ("wwan0".to_string(), "down".to_string()),
            ]
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
