# OES (Orb Event Stream)

Any process on the Orb can publish to OES by using zenorb and publishing to
`oes/<event_name>` with encoding set to `application/json` or
`application/text`. These events are automatically forwarded to the backend.

Events are forwarded to the same backend status endpoint used by
`orb-backend-status` (see `src/backend/status.rs`). The request payload uses
the same `OrbStatusApiV2` schema, but only the `oes` field is populated:

```rust
struct OrbStatusApiV2 {
    oes: Option<Vec<Event>>,
    // ... all other fields omitted (all Optional) ...
}

struct Event {
    name: String,
    created_at: DateTime<Utc>,
    payload: Option<serde_json::Value>,
}
```

The `oes` field is a list of events. Each event has a `name` (e.g.
`"connd/active_connections"`), a `created_at` timestamp, and an optional
JSON `payload`.

---

## Events

### `connd/active_connections`

Published by `orb-connd` whenever the primary network connection changes.
Reports the state of every active NetworkManager connection, including DNS
resolution and an HTTP connectivity check per interface.

```rust
struct ActiveConnections {
    /// The primary connection event that triggered this report.
    primary_connection: PrimaryConnection,
    /// The NetworkManager connectivity check URI.
    connectivity_uri: String,
    /// Hostname extracted from connectivity_uri, used for DNS resolution checks.
    hostname: Option<String>,
    /// One entry per (connection, interface) pair.
    connections: Vec<Connection>,
}

struct Connection {
    /// NetworkManager connection name.
    /// For wifi this is the SSID; for cellular/ethernet it is the NM profile name.
    name: String,
    /// Network interface name:
    ///   - "wlan0" — wifi
    ///   - "wwan0" — cellular
    ///   - "eth0"  — ethernet
    iface: String,
    /// Whether this connection matches the primary_connection.
    primary: bool,
    ipv4_addresses: Vec<String>,
    ipv6_addresses: Vec<String>,
    /// Per-link DNS status from systemd-resolved.
    dns_status: Result<LinkDnsStatus, String>,
    /// Resolution of the connectivity check hostname through this interface.
    dns_resolution: Result<Option<HostnameResolution>, String>,
    /// HTTP GET to the connectivity URI through this interface.
    http_check: Result<HttpCheck, String>,
}

enum PrimaryConnection {
    Disconnected,
    Disconnecting,
    Connecting,
    ConnectedLocal(ConnectionKind),
    ConnectedSite(ConnectionKind),
    ConnectedGlobal(ConnectionKind),
}

enum ConnectionKind {
    Wifi { ssid: String },
    Cellular { apn: String },
    Ethernet,
}

struct HttpCheck {
    /// HTTP status code (e.g. 200, 302, 204).
    status: u16,
    /// Value of the Location header, if present (e.g. on redirects).
    location: Option<String>,
    /// Value of the X-NetworkManager-Status header, if present.
    nm_status: Option<String>,
    content_length: Option<String>,
    /// Round-trip time for the HTTP request.
    elapsed: Duration,
}

struct LinkDnsStatus {
    /// The DNS server currently being used for queries on this link.
    current_dns_server: Option<IpAddr>,
    /// All configured DNS servers on this link.
    dns_servers: Vec<IpAddr>,
    /// Search and routing domains configured on this link.
    domains: Vec<DnsDomain>,
    /// Whether this link is used as the default route for DNS queries.
    default_route: bool,
}

struct DnsDomain {
    domain: String,
    /// Routing-only domain (prefixed with ~ in resolvectl output).
    is_routing_domain: bool,
}

struct HostnameResolution {
    /// IP addresses the hostname resolved to.
    addresses: Vec<IpAddr>,
    /// Canonical hostname returned by the resolver.
    canonical_name: String,
    flags: ResolveFlags,
}

struct ResolveFlags {
    from_cache: bool,
    from_network: bool,
    synthetic: bool,
    from_zone: bool,
    from_trust_anchor: bool,
    /// The data has been fully authenticated (e.g. DNSSEC).
    authenticated: bool,
    /// The query was resolved via encrypted channels or never left this system.
    confidential: bool,
}
```

`dns_status`, `dns_resolution`, and `http_check` are serialized as
`Result<T, String>` — on success the value is present, on failure a
human-readable error string is provided.

#### Example

```json
{
  "primary_connection": { "ConnectedGlobal": "Ethernet" },
  "connectivity_uri": "http://connectivity-check.worldcoin.org",
  "hostname": "connectivity-check.worldcoin.org",
  "connections": [
    {
      "name": "TFHOrbs",
      "iface": "wlan0",
      "primary": false,
      "ipv4_addresses": ["10.108.0.74"],
      "ipv6_addresses": [],
      "dns_status": {
        "Ok": {
          "current_dns_server": "10.108.0.1",
          "default_route": true,
          "dns_servers": ["10.108.0.1"],
          "domains": [
            { "domain": "local.meter", "is_routing_domain": false }
          ]
        }
      },
      "dns_resolution": {
        "Ok": {
          "addresses": [
            "104.18.23.206",
            "104.18.22.206",
            "2606:4700::6812:17ce",
            "2606:4700::6812:16ce"
          ],
          "canonical_name": "connectivity-check.worldcoin.org",
          "flags": {
            "authenticated": false,
            "confidential": false,
            "from_cache": true,
            "from_network": false,
            "from_trust_anchor": false,
            "from_zone": false,
            "synthetic": false
          }
        }
      },
      "http_check": {
        "Ok": {
          "status": 204,
          "location": null,
          "nm_status": "online",
          "content_length": null,
          "elapsed": { "secs": 0, "nanos": 38427572 }
        }
      }
    },
    {
      "name": "Wired connection 1",
      "iface": "eth0",
      "primary": true,
      "ipv4_addresses": ["10.103.0.167", "10.103.0.234"],
      "ipv6_addresses": ["fe80::3e6d:66ff:fe2d:21a2"],
      "dns_status": {
        "Ok": {
          "current_dns_server": "9.9.9.9",
          "default_route": true,
          "dns_servers": ["8.8.4.4", "8.8.8.8", "9.9.9.9", "1.1.1.1"],
          "domains": []
        }
      },
      "dns_resolution": {
        "Ok": {
          "addresses": [
            "2606:4700::6812:16ce",
            "2606:4700::6812:17ce",
            "104.18.22.206",
            "104.18.23.206"
          ],
          "canonical_name": "connectivity-check.worldcoin.org",
          "flags": {
            "authenticated": false,
            "confidential": false,
            "from_cache": false,
            "from_network": true,
            "from_trust_anchor": false,
            "from_zone": false,
            "synthetic": false
          }
        }
      },
      "http_check": {
        "Ok": {
          "status": 204,
          "location": null,
          "nm_status": "online",
          "content_length": null,
          "elapsed": { "secs": 0, "nanos": 8530792 }
        }
      }
    }
  ]
}
```
