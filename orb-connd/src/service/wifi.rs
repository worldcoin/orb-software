use std::collections::HashMap;

use crate::network_manager::WifiSec;
use color_eyre::{eyre::ContextCompat, Result};

#[derive(Debug, PartialEq, Clone)]
pub struct Credentials {
    pub ssid: String,
    pub sec: WifiSec,
    pub psk: Option<String>,
    pub hidden: bool,
}

impl Credentials {
    pub fn parse(wifi_str: &str) -> Result<Credentials> {
        let wifi_str = wifi_str.replace("WIFI:", "");
        let map: HashMap<_, _> = wifi_str
            .split(";")
            .flat_map(|entry| entry.split_once(":"))
            .collect();

        let sec = map
            .get("T")
            .and_then(|sec| WifiSec::parse(sec))
            .wrap_err_with(|| {
                format!("invalid or missing wifi sec on qr code: {wifi_str}")
            })?;

        let ssid = map
            .get("S")
            .wrap_err_with(|| format!("missing wifi ssid on qr code: {wifi_str}"))?
            .to_string();

        let hidden = map
            .get("H")
            .map(|h| h.parse())
            .transpose()?
            .unwrap_or_default();

        let psk = map.get("P").map(|p| p.to_string());

        Ok(Credentials {
            ssid,
            sec,
            psk,
            hidden,
        })
    }
}
