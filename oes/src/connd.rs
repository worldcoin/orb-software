use serde::{Deserialize, Serialize};

/// A snapshot of all currently active network connections on the orb.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveConnections {
    /// The URI used to check if we have internet connectivity.
    pub connectivity_uri: String,
    /// The list of currently active connections.
    pub connections: Vec<Connection>,
}

/// A single active network connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    /// The connection's display name (e.g. "Wired connection 1").
    pub name: String,
    /// The network interface backing this connection.
    pub iface: NetworkInterface,
    /// Whether this is the primary (default-route) connection.
    pub primary: bool,
    /// Whether this connection currently has internet access.
    pub has_internet: bool,
}

/// The network interface used by this connection
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkInterface {
    Ethernet,
    WiFi,
    Cellular,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellularStatus {
    pub imei: String,
    pub fw_revision: Option<String>,
    pub iccid: Option<String>,
    /// Radio Access Technology -- e.g.: gsm, lte
    pub rat: Option<String>,
    pub operator: Option<String>,
    /// Reference Option Received Power — how strong the cellular signal is.
    pub rsrp: Option<f64>,
    ///Reference Signal Received Quality — signal quality, affected by interference.
    pub rsrq: Option<f64>,
    /// Received Signal Strength Indicator — total signal power (including noise)
    pub rssi: Option<f64>,
    /// Signal-to-Noise Ratio — how "clean" the signal is.
    pub snr: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetStats {
    pub iface: String,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}
