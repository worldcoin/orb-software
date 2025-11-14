use async_trait::async_trait;
use color_eyre::Result;

pub mod cli;

#[derive(Debug, Clone, PartialEq)]
pub struct AccessPoint {
    pub bssid: String,
    pub ssid: Option<String>,
    pub rssi: i32,
}

#[async_trait]
pub trait WpaCtrl: 'static + Send + Sync {
    async fn scan_results(&self) -> Result<Vec<AccessPoint>>;
}
