use super::net_stats::NetStats;
use crate::modem_manager::{
    connection_state::ConnectionState, Location, ModemId, Signal,
};

pub struct ModemStatus {
    pub id: ModemId,
    pub iccid: Option<String>,
    pub imei: String,
    /// Radio Access Technology -- e.g.: gsm, lte
    pub rat: Option<String>,
    pub operator: Option<String>,
    pub state: ConnectionState,
    pub signal: Signal,
    pub location: Location,
    pub net_stats: NetStats,
}

impl ModemStatus {
    pub fn new(
        id: ModemId,
        iccid: Option<String>,
        imei: String,
        state: ConnectionState,
        net_stats: NetStats,
    ) -> Self {
        Self {
            id,
            iccid,
            imei,
            rat: None,
            operator: None,
            state,
            signal: Signal::default(),
            location: Location::default(),
            net_stats,
        }
    }
}
