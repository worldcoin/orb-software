pub mod agent;
pub(crate) mod handler;

use std::{
    collections::HashMap,
    net::{Ipv6Addr, SocketAddrV6},
};

use eyre::Result;
use eyre::WrapErr as _;
use iroh::{endpoint::ConnectionType, protocol::ProtocolHandler, Endpoint};

pub use crate::agent::Agent;
pub use handler::BoxedHandler;
pub type ConnectionTypeWatcher = iroh::watchable::Watcher<ConnectionType>;

#[derive(Debug, Eq, Hash, Clone, Copy, derive_more::Display)]
pub struct Alpn(pub &'static str);

impl AsRef<str> for Alpn {
    fn as_ref(&self) -> &str {
        self.0
    }
}

impl AsRef<[u8]> for Alpn {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl<T: AsRef<[u8]>> PartialEq<T> for Alpn {
    fn eq(&self, other: &T) -> bool {
        self.0.as_bytes() == other.as_ref()
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct FromAnyhow(#[from] pub anyhow::Error);

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct FromEyre(#[from] pub eyre::Report);

/// Configures which discovery service providers are enabled.
///
/// # What is Discovery?
///
/// In iroh, discovery is done via services called [pkarr relays][pkarr], whose job is
/// to map from public keys to DNS records. These DNS records can then contain IP
/// addresses, service locations, and various other metadata. Iroh refers to this
/// process of mapping from pubkey -> ip addresses/ iroh relay addresses as "discovery".
///
/// Note that the concept of "discovery" in iroh is distinct from the separate concept
/// of "relays". The former is for finding metadata such as IP addresses from pubkeys,
/// the latter is actually directly facilitating connections.
///
/// At least one discovery service must be enabled, unless you are relying *entirely*
/// on mDNS and can assume a shared LAN, in which case the DNS packets can be broadcast
/// enirely over udp multicast, and a service is not needed.
///
/// # Trust model
///
/// Data provided by these services is trustless, because lookups are done by ed25519
/// pubkey, and the data returned is rejected if not signed by that pubkey. Therefore
/// a relay can be operated by untrusted third parties, and data is tamper-proof.
///
/// It is possible however for a relay to serve out of date valid data, or to otherwise
/// engage in denial-of-service, or simply reject requests. For this reason, ideally
/// multiple discovery service providers should be used.
///
/// # Network topology.
///
/// Discovery services are typically peer to peer "supernodes", i.e. nodes that are:
/// * publicly addressed without NAT/Port forwarding.
/// * used to facilitate other weaker regular nodes
///
/// The nodes use the bittorrent mainline DHT as their overlay network, and publish
/// information to the DHT. Unlike a blockchain, this data is highly efficient and
/// also ephemeral - it is not a ledger, more like a mutable set. If a relay stops
/// publishing data, that data is dropped from the DHT after approximately one hour.
///
/// # Efficiency
///
/// PKARR relays are extremely scalable. The data model that they use is just signed
/// DNS packets under the hood, so they are similarly scalable as DNS. In fact, many
/// if not most PKARR relays simultaneously operate as DNS servers.
///
/// PKARR is NOT intended as a realtime store of data! Records are, like DNS, intended
/// to be rarely modified, and modifications do not propagate in real-time, it may take
/// a few seconds to look data up if there is a cache miss. To alleviate this, PKARR
/// heavily leverages caching, by using the DNS packet's TTL (time-to-live) setting
/// field to control how often to retrieve data, and enable clients to cache data.
/// It is also for this reason that orb-agent-iroh keeps the endpoint initialized even
/// when not actively in use, to guarantee that the PKARR relay can make the node's
/// location available on the DHT even before clients connect.
///
/// If two clients request data from the same relay, this data is going to be cached,
/// so in practice, the Worldcoin Foundation should operate one or more relays that
/// all orbs and apps talk on, to minimize latency. Other third parties will still be
/// able to talk too, and they can also host their own relays.
///
/// [pkarr]: https://github.com/pubky/pkarr
#[derive(Debug, Clone)]
pub struct EnabledDiscoveryServices {
    /// Operated by <https://n0.computer>.
    pub n0: bool,
    /// Operated by the Worldcoin Foundation.
    pub world: bool,
}

impl Default for EnabledDiscoveryServices {
    fn default() -> Self {
        Self {
            n0: true,
            world: false,
        }
    }
}

#[derive(Debug, bon::Builder)]
pub struct RouterConfig {
    #[builder(field)]
    pub handlers: HashMap<Alpn, Box<dyn ProtocolHandler>>,
}

impl<S: router_config_builder::State> RouterConfigBuilder<S> {
    pub fn handler<T: ProtocolHandler>(
        mut self,
        alpn: impl Into<Alpn>,
        router: impl Into<Box<T>>,
    ) -> Self {
        self.handlers.insert(alpn.into(), router.into());
        self
    }
}

#[derive(Debug, bon::Builder)]
pub struct EndpointConfig {
    /// Provide a specific secret key for mTLS, instead of dynamically generating one
    secret_key: Option<iroh::SecretKey>,
    #[builder(default)]
    discovery: EnabledDiscoveryServices,
    /// in QUIC, the ALPN allows for multiple connections on same port, and it is ideal
    /// to circumvent firewalls to always use port 443 (same as https). You can also set
    /// this to 0 to dynamically choose a port.
    #[builder(default = 443)]
    port: u16,
}

impl EndpointConfig {
    pub async fn bind(self) -> Result<Endpoint> {
        // IPV6 is preferable as it has a higher chance of penetrating NAT
        // Typically linux will dual-bind to ipv4, to allow ipv4-only networks to
        // connect as well. We should double check this.
        let bind_addr = SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, self.port, 0, 0);
        let endpoint = iroh::Endpoint::builder().bind_addr_v6(bind_addr);
        let endpoint = if self.discovery.n0 {
            endpoint.discovery_n0()
        } else {
            endpoint
        };
        if self.discovery.world {
            todo!("we don't yet host PKARR relays! we are working on it :)");
        }

        let endpoint = if let Some(sk) = self.secret_key {
            endpoint.secret_key(sk)
        } else {
            endpoint
        };
        let endpoint = endpoint
            .bind()
            .await
            .map_err(FromAnyhow)
            .wrap_err("failed to bind iroh endpoint")?;

        Ok(endpoint)
    }
}
