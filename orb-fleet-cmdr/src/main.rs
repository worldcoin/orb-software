use clap::Parser;
use std::{borrow::Cow, path::Path};
use tracing::{debug, info};

use fleet_cmdr::{args::Args, settings::Settings};

const CFG_DEFAULT_PATH: &str = "/etc/orb_fleet_cmdr.conf";
const ENV_VAR_PREFIX: &str = "ORB_FLEET_CMDR_";
const CFG_ENV_VAR: &str = const_format::concatcp!(ENV_VAR_PREFIX, "CONFIG");
const SYSLOG_IDENTIFIER: &str = "worldcoin-fleet-cmdr";

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let args = Args::parse();
    let config_path = get_config_source(&args);

    let settings = Settings::get(&args, config_path, ENV_VAR_PREFIX)?;

    debug!(?settings, "starting fleet commander with settings");
    run(&settings)
}

fn get_config_source(args: &Args) -> Cow<'_, Path> {
    if let Some(config) = &args.config {
        info!("using config provided by command line argument: `{config}`");
        Cow::Borrowed(config.as_ref())
    } else if let Some(config) = figment::providers::Env::var(CFG_ENV_VAR) {
        info!("using config set in environment variable `{CFG_ENV_VAR}={config}`");
        Cow::Owned(std::path::PathBuf::from(config))
    } else {
        info!("using default config at `{CFG_DEFAULT_PATH}`");
        Cow::Borrowed(CFG_DEFAULT_PATH.as_ref())
    }
}

fn run(settings: &Settings) -> color_eyre::Result<()> {
    Ok(())
}
