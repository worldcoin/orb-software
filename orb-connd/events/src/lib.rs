use rkyv::{Archive, CheckBytes, Deserialize, Serialize};

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
#[archive_attr(derive(CheckBytes, Debug, PartialEq))]
pub enum Connection {
    /// There is no active network connection.
    Disconnected,
    /// Network connections are being cleaned up.
    Disconnecting,
    /// A network connection is being started.
    Connecting,
    /// There is only local IPv4 and/or IPv6 connectivity,
    /// but no default route to access the Internet.
    ConnectedLocal(ConnectionKind),
    /// There is only site-wide IPv4 and/or IPv6 connectivity.
    /// This means a default route is available, but the Internet connectivity check
    /// (see "Connectivity" property) did not succeed.
    ConnectedSite(ConnectionKind),
    /// There is global IPv4 and/or IPv6 Internet connectivity.
    /// This means the Internet connectivity check succeeded and we have
    /// full network connectivity.
    ConnectedGlobal(ConnectionKind),
}

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
#[archive_attr(derive(CheckBytes, Debug, PartialEq))]
pub enum ConnectionKind {
    Wifi { ssid: String },
    Cellular { apn: String },
    Ethernet,
}
