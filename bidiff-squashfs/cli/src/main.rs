use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use color_eyre::Result;

use orb_build_info::{make_build_info, BuildInfo};
use tokio_util::sync::CancellationToken;

const BUILD_INFO: BuildInfo = make_build_info!();

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}
#[derive(Debug, Parser)]
#[clap(
    author,
    about,
    version = BUILD_INFO.version,
    styles = clap_v3_styles(),
)]
struct Args {
    #[command(subcommand)]
    subcommand: orb_bidiff_squashfs_cli::Args,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let cancel = CancellationToken::new();
    tokio::task::spawn(handle_ctrlc(cancel.clone()));

    let result = args.subcommand.run(cancel.clone()).await;
    let telemetry_flusher = orb_telemetry::TelemetryConfig::new().init();

    telemetry_flusher.flush().await;

    result
}
async fn handle_ctrlc(cancel: CancellationToken) {
    let _guard = cancel.drop_guard();
    let _ = tokio::signal::ctrl_c().await;
}
