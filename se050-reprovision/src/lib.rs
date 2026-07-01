#![forbid(unsafe_code)]

pub mod cli;
pub mod remote_api;

pub use orb_se050_reprovision_validate as validate;

use color_eyre::{eyre::WrapErr as _, Result};
use orb_build_info::{make_build_info, BuildInfo};
use rand::{rngs::StdRng, Rng as _};
use tracing::info;

use crate::cli::{CliStrategy, Nonce};

pub const SYSLOG_IDENTIFIER: &str = "worldcoin-se050-reprovision";
pub const BUILD_INFO: BuildInfo = make_build_info!();

#[derive(Debug)]
pub struct Config {
    pub rng: StdRng,
    pub client: crate::remote_api::Client,
    /// Path to the CA that performs the re-enrollment
    pub cli_strat: CliStrategy,
}

pub async fn run(mut cfg: Config) -> Result<()> {
    info!("orb-se050-reprovision version {}", BUILD_INFO.version);

    // TODO: Make this code not dummy stubbed code. For now we just call the reprovision
    // CLI with some bogus nonce.
    let mut nonce = Nonce::default();
    cfg.rng.fill(&mut nonce);
    let output = crate::cli::call(cfg.cli_strat, nonce)
        .await
        .wrap_err("failed to call cli")?;
    info!("cli output: {output:?}");

    Ok(())
}
