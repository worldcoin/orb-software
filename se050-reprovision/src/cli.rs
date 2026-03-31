use std::process::Stdio;

use color_eyre::eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::Config;

#[derive(Debug, Serialize, Deserialize)]
pub struct CliOutput {
    jetson_authkey: KeyInfo,
    attestation_key: KeyInfo,
    iris_code_key: KeyInfo,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeyInfo {
    /// PEM format
    key: String,
    #[serde(with = "crate::base64_serde")]
    signature: Vec<u8>,
    #[serde(with = "crate::base64_serde")]
    extra_data: Vec<u8>,
    // active: bool,
}

pub async fn call(cfg: &Config, nonce: u128) -> Result<CliOutput> {
    let mut child = tokio::process::Command::new(&cfg.ca_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .wrap_err_with(|| format!("failed to spawn {}", cfg.ca_path.display()))?;
    let mut stdin = child.stdin.take().expect("infallible");
    let mut stdout = child.stdout.take().expect("infallible");

    stdin
        .write_all(&nonce.to_be_bytes())
        .await
        .wrap_err("failed to write nonce to stdin")?;
    stdin.shutdown().await?;
    drop(stdin);

    let mut output = String::new();
    stdout
        .read_to_string(&mut output)
        .await
        .wrap_err("failed to read from stdout")?;

    serde_json::from_str(&output).wrap_err("failed to deserialize stdout as json")
}
