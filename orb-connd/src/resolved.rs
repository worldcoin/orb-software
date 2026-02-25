use color_eyre::{eyre::Context, Result};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use tracing::warn;
use zbus_systemd::resolve1;

/// Client for systemd-resolved, providing DNS resolution and resolver status
/// via the `org.freedesktop.resolve1` D-Bus interface.
#[derive(Clone)]
pub struct Resolved {
    conn: zbus::Connection,
}

impl Resolved {
    pub fn new(conn: zbus::Connection) -> Self {
        Self { conn }
    }

    pub async fn resolve_hostname(
        &self,
        iface: &str,
        hostname: &str,
    ) -> Result<HostnameResolution> {
        let ifindex = nix::net::if_::if_nametoindex(iface)
            .wrap_err_with(|| format!("unknown interface: {iface}"))?
            as i32;
        let proxy = resolve1::ManagerProxy::new(&self.conn).await?;
        let (raw_addrs, canonical_name, flags) = proxy
            .resolve_hostname(ifindex, hostname.to_string(), 0, 0)
            .await?;

        let addresses = raw_addrs
            .into_iter()
            .filter_map(|(_ifindex, family, bytes)| parse_ip(family, &bytes))
            .collect();

        Ok(HostnameResolution {
            addresses,
            canonical_name,
            flags: ResolveFlags::from_raw(flags),
        })
    }

    pub async fn link_status(&self, iface: &str) -> Result<LinkDnsStatus> {
        let ifindex = nix::net::if_::if_nametoindex(iface)
            .wrap_err_with(|| format!("unknown interface: {iface}"))?
            as i32;
        let manager = resolve1::ManagerProxy::new(&self.conn).await?;
        let link_path = manager.get_link(ifindex).await?;
        let link = resolve1::LinkProxy::builder(&self.conn)
            .path(link_path)?
            .build()
            .await?;

        let current_dns_server = link
            .current_dns_server()
            .await
            .inspect_err(|e| warn!("failed to get current DNS server for {iface}: {e}"))
            .ok()
            .and_then(|(family, bytes)| parse_ip(family, &bytes));

        let dns_servers = link
            .dns()
            .await?
            .into_iter()
            .filter_map(|(family, bytes)| parse_ip(family, &bytes))
            .collect();

        let domains = link
            .domains()
            .await?
            .into_iter()
            .map(|(domain, is_routing_domain)| DnsDomain {
                domain,
                is_routing_domain,
            })
            .collect();

        let default_route = link
            .default_route()
            .await
            .inspect_err(|e| warn!("failed to get default route for {iface}: {e}"))
            .unwrap_or(false);

        Ok(LinkDnsStatus {
            current_dns_server,
            dns_servers,
            domains,
            default_route,
        })
    }
}

/// AF_INET from <sys/socket.h>
const AF_INET: i32 = 2;
/// AF_INET6 from <sys/socket.h>
const AF_INET6: i32 = 10;

fn parse_ip(family: i32, bytes: &[u8]) -> Option<IpAddr> {
    match family {
        AF_INET if bytes.len() == 4 => {
            let octets: [u8; 4] = bytes.try_into().ok()?;
            Some(IpAddr::V4(Ipv4Addr::from(octets)))
        }

        AF_INET6 if bytes.len() == 16 => {
            let octets: [u8; 16] = bytes.try_into().ok()?;
            Some(IpAddr::V6(Ipv6Addr::from(octets)))
        }

        _ => None,
    }
}

/// Result of resolving a hostname via systemd-resolved.
#[derive(Debug, serde::Serialize)]
pub struct HostnameResolution {
    /// IP addresses the hostname resolved to.
    pub addresses: Vec<IpAddr>,
    /// Canonical hostname returned by the resolver.
    pub canonical_name: String,
    /// Flags indicating origin and security properties of the response.
    pub flags: ResolveFlags,
}

/// Flags returned by systemd-resolved indicating how a query was answered.
///
/// Bit positions sourced from:
/// <https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.resolve1.html>
#[derive(Debug, serde::Serialize)]
pub struct ResolveFlags {
    /// The answer came (at least partially) from the local cache.
    pub from_cache: bool,
    /// The answer came (at least partially) from the network.
    pub from_network: bool,
    /// The answer was (at least partially) synthesized locally.
    pub synthetic: bool,
    /// The answer came (at least partially) from a locally registered zone.
    pub from_zone: bool,
    /// The answer came (at least partially) from local trust anchors.
    pub from_trust_anchor: bool,
    /// The returned data has been fully authenticated (e.g. DNSSEC).
    pub authenticated: bool,
    /// The query was resolved via encrypted channels or never left this
    /// system.
    pub confidential: bool,
}

impl ResolveFlags {
    fn from_raw(flags: u64) -> Self {
        Self {
            from_cache: flags & (1 << 20) != 0,
            from_network: flags & (1 << 23) != 0,
            synthetic: flags & (1 << 19) != 0,
            from_zone: flags & (1 << 21) != 0,
            from_trust_anchor: flags & (1 << 22) != 0,
            authenticated: flags & (1 << 9) != 0,
            confidential: flags & (1 << 18) != 0,
        }
    }
}

/// A search or routing domain configured in systemd-resolved.
#[derive(Debug, serde::Serialize)]
pub struct DnsDomain {
    /// The domain name.
    pub domain: String,
    /// If true, this is a routing-only domain (prefixed with `~` in
    /// `resolvectl status` output) used to route queries to specific DNS
    /// servers without being used for search.
    pub is_routing_domain: bool,
}

/// Per-link DNS status from systemd-resolved, equivalent to the per-link
/// section of `resolvectl status`.
#[derive(Debug, serde::Serialize)]
pub struct LinkDnsStatus {
    /// The DNS server currently being used for queries on this link.
    pub current_dns_server: Option<IpAddr>,
    /// All configured DNS servers on this link.
    pub dns_servers: Vec<IpAddr>,
    /// Search and routing domains configured on this link.
    pub domains: Vec<DnsDomain>,
    /// Whether this link is used as the default route for DNS queries.
    pub default_route: bool,
}
