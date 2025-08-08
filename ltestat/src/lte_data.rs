use chrono::Utc;
use color_eyre::Result;
use serde::{Deserialize, Serialize};

pub struct LteStat {
    /// Connected to network via LTE
    connected: bool,

    /// Parsed `mmcli` output
    current_stat: LteMetrics,

    /// Timestamp when dissconnected
    dissconnected_: chrono::DateTime<Utc>,
}

pub struct LteMetrics {
    pub rssi: Option<i32>,
}
