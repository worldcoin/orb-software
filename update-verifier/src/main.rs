use std::env;
use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use color_eyre::eyre::{self, Context};
use orb_slot_ctrl::{EfiVarDb, OrbSlotCtrl};
use orb_update_verifier::BUILD_INFO;
use tracing::error;

const SYSLOG_IDENTIFIER: &str = "worldcoin-update-verifier";

#[derive(Parser, Debug)]
#[clap(
    version = BUILD_INFO.version,
    about,
    styles = clap_v3_styles(),
)]
struct Cli {}

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let otel_config = orb_telemetry::OpenTelemetryConfig::new(
        "http://localhost:4317",
        SYSLOG_IDENTIFIER,
        BUILD_INFO.version,
        env::var("ORB_BACKEND").expect("ORB_BACKEND environment variable must be set").to_lowercase(),
    );

    let _telemetry_guard = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .with_opentelemetry(otel_config)
        .init();

    run().inspect_err(|error| error!(?error, "failed to run update-verifier"))
}

fn run() -> eyre::Result<()> {
    let _args = Cli::parse();

    let efi_var_db = EfiVarDb::from_rootfs("/")?;
    let orb_slot_ctrl = OrbSlotCtrl::new(&efi_var_db)?;
    orb_update_verifier::run_health_check(orb_slot_ctrl)
        .wrap_err("update verifier encountered error while checking system health")?;

    Ok(())
}
