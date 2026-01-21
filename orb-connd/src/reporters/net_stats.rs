use color_eyre::Result;
use serde::Serialize;
use std::path::Path;
use tokio::fs;

#[derive(Debug, Serialize, Clone)]
pub struct NetStats {
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}

impl NetStats {
    pub async fn collect(sysfs: impl AsRef<Path>, iface: &str) -> Result<NetStats> {
        let stats_path = sysfs
            .as_ref()
            .join("class")
            .join("net")
            .join(iface)
            .join("statistics");

        let tx_path = stats_path.join("tx_bytes");
        let rx_path = stats_path.join("rx_bytes");

        let tx_bytes = String::from_utf8_lossy(&fs::read(tx_path).await?)
            .trim()
            .parse()?;

        let rx_bytes = String::from_utf8_lossy(&fs::read(rx_path).await?)
            .trim()
            .parse()?;

        Ok(NetStats { tx_bytes, rx_bytes })
    }
}
