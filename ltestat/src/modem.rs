use crate::{
    connection_state::ConnectionState, lte_data::LteStat, modem_manager::ModemInfo,
};
use color_eyre::Result;

pub struct Modem {
    pub id: String,
    pub iccid: String,
    pub imei: String,
    pub rat: Option<String>,
    pub operator: Option<String>,

    pub state: ConnectionState,
    pub last_state: Option<ConnectionState>,
    pub disconnected_count: u64,
    pub last_snapshot: Option<LteStat>,
}

impl Modem {
    pub fn new(id: String, iccid: String, imei: String) -> Result<Self> {
        Ok(Self {
            id,
            iccid,
            imei,
            rat: None,
            operator: None,
            state: ConnectionState::Unknown,
            last_state: None,
            disconnected_count: 0,
            last_snapshot: None,
        })
    }
}
