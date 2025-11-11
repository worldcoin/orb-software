use color_eyre::{
    eyre::{bail, ContextCompat},
    Result,
};
use std::collections::HashMap;
use tokio::{fs::File, io::AsyncReadExt};
use tracing::{info, warn};

pub struct LegacyWpaConfig {
    pub ssid: String,
    pub psk: String,
}

impl LegacyWpaConfig {
    pub async fn from_file(mut file: File) -> Result<LegacyWpaConfig> {
        let mut contents = String::new();
        file.read_to_string(&mut contents).await?;

        let map: HashMap<_, _> = contents
            .lines()
            .filter_map(|line| line.trim().split_once("="))
            .collect();

        let ssid = map.get("ssid").wrap_err("could not parse ssid")?;
        let ssid = normalize_wpa_ssid(ssid)?;

        let psk = map.get("psk").wrap_err("could not parse psk")?;

        // Validate PSK is not empty
        // psk can also be quoted or not, probably should normalize as well
        // Check if orb-core might ever done that

        if psk.is_empty() {
            bail!("PSK cannot be empty");
        }

        Ok(LegacyWpaConfig {
            ssid,
            psk: psk.to_string(),
        })
    }
}

fn normalize_wpa_ssid(ssid_raw: &str) -> Result<String> {
    // Is quoted = regular SSID string, pass it down as String
    let is_quoted =
        ssid_raw.len() >= 2 && ssid_raw.starts_with('"') && ssid_raw.ends_with('"');

    if is_quoted {
        let unquoted = &ssid_raw[1..ssid_raw.len() - 1];
        validate_len(unquoted.len())?;

        return Ok(unquoted.to_owned());
    }

    // SSID was not quoted -> handle hex variant
    let ssid_bytes = match hex::decode(ssid_raw) {
        Ok(bytes) => {
            validate_len(bytes.len())?;
            bytes
        }

        Err(e) => {
            warn!("failed to decode hex SSID: {e}, treating as raw string");
            validate_len(ssid_raw.len())?;

            return Ok(ssid_raw.to_owned());
        }
    };

    let ssid_string = match String::from_utf8(ssid_bytes) {
        Ok(decoded) => {
            info!("decoded hex-encoded SSID: {ssid_raw} -> {decoded}");
            decoded
        }

        Err(e) => {
            warn!("hex-encoded SSID is not valid UTF-8: {e}, treating as raw string");
            validate_len(ssid_raw.len())?;

            return Ok(ssid_raw.to_owned());
        }
    };

    Ok(ssid_string)
}

fn validate_len(len: usize) -> Result<()> {
    if len > 32 {
        bail!("SSID too long: {len} bytes (max 32)");
    }

    if len == 0 {
        bail!("SSID cannot be empty");
    }

    Ok(())
}
