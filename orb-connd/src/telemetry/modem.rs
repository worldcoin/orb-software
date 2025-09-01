use super::{
    connection_state::ConnectionState, location::GppLocation, net_stats::NetStats,
    signal::LteSignal,
};

pub struct Modem {
    pub id: String,
    pub iccid: String,
    pub imei: String,
    /// Radio Access Technology -- e.g.: gsm, lte
    pub rat: Option<String>,
    pub operator: Option<String>,

    pub state: ConnectionState,
    pub prev_state: Option<ConnectionState>,
    pub disconnected_count: u64,

    pub signal: Option<LteSignal>,
    pub location: Option<GppLocation>,
    pub net_stats: NetStats,
}

impl Modem {
    pub fn new(
        id: String,
        iccid: String,
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
            prev_state: None,
            disconnected_count: 0,
            signal: None,
            location: None,
            net_stats,
        }
    }
}
