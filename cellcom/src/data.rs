use serde::{Deserialize, Serialize};

use crate::cell::{NeighborCell, ServingCell};

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub wifi: Vec<WifiNetwork>,
    pub cellular: CellularInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WifiNetwork {
    pub bssid: String,
    pub frequency: u32,
    pub signal_level: i32,
    pub flags: String,
    pub ssid: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CellularInfo {
    pub serving_cell: ServingCell,
    pub neighbor_cells: Vec<NeighborCell>,
}
