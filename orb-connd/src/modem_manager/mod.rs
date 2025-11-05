use async_trait::async_trait;
use color_eyre::{eyre, Result};
use connection_state::ConnectionState;
use serde::{Deserialize, Serialize};
use std::{str::FromStr, time::Duration};

pub mod cli;
pub mod connection_state;

#[async_trait]
pub trait ModemManager: 'static + Send + Sync {
    async fn list_modems(&self) -> Result<Vec<Modem>>;

    async fn modem_info(&self, modem_id: &ModemId) -> Result<ModemInfo>;

    async fn signal_setup(&self, modem_id: &ModemId, rate: Duration) -> Result<()>;

    async fn signal_get(&self, modem_id: &ModemId) -> Result<Signal>;

    async fn location_get(&self, modem_id: &ModemId) -> Result<Location>;

    async fn sim_info(&self, sim_id: &SimId) -> Result<SimInfo>;

    async fn set_current_bands<'a>(
        &self,
        modem_id: &ModemId,
        bands: &[&'a str],
    ) -> Result<()>;

    async fn set_allowed_and_preferred_modes<'a>(
        &self,
        modem_id: &ModemId,
        allowed: &[&'a str],
        preferred: &[&'a str],
    ) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModemInfo {
    pub imei: String,
    pub operator_code: Option<String>,
    pub operator_name: Option<String>,
    pub access_tech: Option<String>,
    pub state: ConnectionState,
    pub sim: Option<SimId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModemId(String);

impl From<usize> for ModemId {
    fn from(value: usize) -> Self {
        ModemId(value.to_string())
    }
}

impl FromStr for ModemId {
    type Err = eyre::Report;

    fn from_str(id: &str) -> std::result::Result<Self, Self::Err> {
        let _: usize = id.parse()?;

        Ok(ModemId(id.to_string()))
    }
}

impl ModemId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SimId(String);

impl From<usize> for SimId {
    fn from(value: usize) -> Self {
        SimId(value.to_string())
    }
}

impl FromStr for SimId {
    type Err = eyre::Report;

    fn from_str(id: &str) -> std::result::Result<Self, Self::Err> {
        let _: usize = id.parse()?;

        Ok(SimId(id.to_string()))
    }
}

impl SimId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Modem {
    pub id: ModemId,
    pub vendor: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SimInfo {
    pub iccid: String,
    pub imsi: String,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Signal {
    /// Reference Signal Received Power — how strong the cellular signal is.
    pub rsrp: Option<f64>,

    ///Reference Signal Received Quality — signal quality, affected by interference.
    pub rsrq: Option<f64>,

    /// Received Signal Strength Indicator — total signal power (including noise)
    pub rssi: Option<f64>,

    /// Signal-to-Noise Ratio) — how "clean" the signal is.
    pub snr: Option<f64>,
}

/// Information about the currently connected cell tower.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Location {
    /// Cell ID — unique identifier for the specific cell tower sector.
    pub cid: Option<String>,

    /// Location Area Code — identifies a group of nearby towers.
    pub lac: Option<String>,

    /// Mobile Country Code — identifies the country.
    pub mcc: Option<String>,

    /// Mobile Network Code — identifies the carrier.
    pub mnc: Option<String>,

    /// Tracking Area Code — like LAC, but specific to LTE.
    pub tac: Option<String>,
}
