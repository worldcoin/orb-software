//! This module contains types that are not part of google's api but are used in builders for the
//! google types.

use orb_cellcom::{NeighborCell, ServingCell};

#[derive(Debug)]
pub struct NetworkInfo {
    pub wifi: Vec<WifiNetwork>,
    pub cellular: CellularInfo,
}

#[derive(Debug)]
pub struct WifiNetwork {
    pub bssid: String,
    pub frequency: u32,
    pub signal_level: i32,
    pub flags: String,
    pub ssid: String,
}

#[derive(Debug)]
pub struct CellularInfo {
    pub serving_cell: ServingCell,
    pub neighbor_cells: Vec<NeighborCell>,
}
