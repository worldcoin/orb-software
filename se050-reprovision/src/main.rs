use std::path::PathBuf;

use clap::Parser;
use color_eyre::{eyre::WrapErr as _, Result};
use orb_endpoints::Backend;
use orb_se050_reprovision::{cli::CliStrategy, Config, BUILD_INFO};
use rand::{rngs::StdRng, SeedableRng};

#[derive(Debug, Parser)]
#[clap(version = BUILD_INFO.version, about)]
pub struct Args {}

impl Args {
    fn make_config(self, backend: Backend) -> Result<Config> {
        Ok(Config {
            client: orb_se050_reprovision::remote_api::Client::builder()
                .default_reqwest_client()?
                .from_backend(backend)
                .build(),
            cli_strat: CliStrategy::Process(PathBuf::from(
                "/usr/local/bin/orb-se050-reprovision-ca",
            )),
            rng: StdRng::from_entropy(),
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let telemetry = orb_telemetry::TelemetryConfig::new()
        .with_journald(orb_se050_reprovision::SYSLOG_IDENTIFIER)
        .init();

    let args = Args::parse();
    let backend =
        Backend::from_env().wrap_err("failed to determine backend from env var")?;
    let config = args
        .make_config(backend)
        .wrap_err("failed to create config")?;
    let result = orb_se050_reprovision::run(config).await;

    telemetry.flush().await;
    result
}
