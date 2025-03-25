use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServingCell {
    /// "State" of the UE (SEARCH, LIMSRV, NOCONN, CONNECT)
    pub connection_status: String,
    /// RAT from ("GSM", "WCDMA", "LTE", "CDMA", "HDR", "TDSCDMA")
    pub network_type: String,

    /// Duplex mode
    /// - "FDD"
    /// - "TDD"
    /// - "-"
    ///
    /// Only applies  for LTE RAT
    pub duplex_mode: String,

    /// Mobile Country Code
    /// (common across RATs)
    pub mcc: Option<u32>,
    /// Mobile Network Code
    /// (common across RATs)
    pub mnc: Option<u32>,

    /// Hex-encoded cell ID
    ///
    /// Based off of RAT:
    /// - LTE:      eNB ID + Cell ID
    /// - WCDMA:    RNC ID + Cell ID
    /// - GSM:      LAC    + Cell ID
    pub cell_id: String,

    /// Channel number or ARFCN (Absolute Radio Frequency Channel Number)
    ///
    /// Actual source field based off RAT:
    /// - LTE:      EARFCN
    /// - WCDMA:    UARFCN
    /// - GSM:      ARFCN
    pub channel_or_arfcn: Option<u32>,

    /// PCID (Physical Cell ID) or PSC (Primary Scrambling Code)
    ///
    /// These play similar roles overall in identifying cells on the same carrier
    /// - LTE:      PCID
    /// - WCDMA:    PSC
    /// - GSM:      PSC
    pub pcid_or_psc: Option<u32>,

    // Basic signal metrics
    pub rsrp: Option<i32>,
    pub rsrq: Option<i32>,
    pub rssi: Option<i32>,
    pub sinr: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NeighborCell {
    pub network_type: String,
    pub channel_or_arfcn: Option<u32>,
    pub pcid_or_psc: Option<u32>,

    pub rsrp: Option<i32>,
    pub rsrq: Option<i32>,
    pub rssi: Option<i32>,
    pub sinr: Option<i32>,
}
