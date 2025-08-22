use crate::utils::run_cmd;
use color_eyre::{eyre::eyre, Result};
use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
pub struct NetStats {
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}

impl NetStats {
    pub async fn from_wwan0() -> Result<Self> {
        let output = run_cmd(
            "cat",
            &[
                "/sys/class/net/wwan0/statistics/tx_bytes",
                "/sys/class/net/wwan0/statistics/rx_bytes",
            ],
        )
        .await?;

        let mut lines = output.lines();

        let tx_bytes: u64 = lines
            .next()
            .ok_or_else(|| eyre!("Missing tx_bytes info line."))?
            .trim()
            .parse()?;

        let rx_bytes: u64 = lines
            .next()
            .ok_or_else(|| eyre!("Missing rx_bytes info line"))?
            .trim()
            .parse()?;

        Ok(Self { tx_bytes, rx_bytes })
    }
}
