use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use color_eyre::eyre::{self, Context};
use orb_info::orb_os_release::OrbOsRelease;
use orb_slot_ctrl::OrbSlotCtrl;
use orb_update_verifier::{run, BUILD_INFO};
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
    let telemetry = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();
    let result =
        run().inspect_err(|error| error!(?error, "failed to run update-verifier"));
    telemetry.flush_blocking();
    result
}
