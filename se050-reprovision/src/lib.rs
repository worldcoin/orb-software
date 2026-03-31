#![forbid(unsafe_code)]

mod cli;
pub mod remote_api;

use std::path::PathBuf;

use color_eyre::{eyre::WrapErr as _, Result};
use orb_build_info::{make_build_info, BuildInfo};
use rand::{rngs::StdRng, RngCore};
use tracing::info;

pub const SYSLOG_IDENTIFIER: &str = "worldcoin-se050-reprovision";
pub const BUILD_INFO: BuildInfo = make_build_info!();

#[derive(Debug, Clone)]
pub struct Config {
    pub rng: StdRng,
    pub client: crate::remote_api::Client,
    /// Path to the CA that performs the re-enrollment
    pub ca_path: PathBuf,
}

pub async fn run(mut cfg: Config) -> Result<()> {
    info!("orb-se050-reprovision version {}", BUILD_INFO.version);

    // TODO: Make this code not dummy stubbed code. For now we just call the reprovision
    // CLI with some bogus nonce.
    let mut nonce = [0; 16];
    cfg.rng.fill_bytes(&mut nonce);
    let nonce = u128::from_le_bytes(nonce);
    let output = crate::cli::call(&cfg, nonce)
        .await
        .wrap_err("failed to call cli");
    std::future::pending::<()>().await;
    let output = output?;
    info!("cli output: {output:?}");

    Ok(())
}

mod base64_serde {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let encoded = STANDARD.encode(value);
        serializer.serialize_str(&encoded)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        STANDARD
            .decode(encoded.as_bytes())
            .map_err(serde::de::Error::custom)
    }
}
