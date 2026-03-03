/// A snapshot of all currently active network connections on the orb.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ActiveConnections {
    /// The URI used to check if we have internet connectivity.
    pub connectivity_uri: String,
    /// The list of currently active connections.
    pub connections: Vec<Connection>,
}

/// A single active network connection.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Connection {
    /// The connection's display name (e.g. "Wired connection 1").
    pub name: String,
    /// The network interface backing this connection.
    ///
    /// - `eth*` -> Ethernet
    /// - `wlan*` -> WiFi
    /// - `wwan*` -> Cellular
    ///
    /// e.g.: `wlan0`, `eth0`, etc
    pub iface: String,
    /// Whether this is the primary (default-route) connection.
    pub primary: bool,
    /// Whether this connection currently has internet access.
    pub has_internet: bool,
}
