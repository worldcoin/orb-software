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

/// A one-stop shop for the orb's developer tooling.
///
/// Each subcommand is its own CLI tool, busybox style.
#[derive(Debug, Parser)]
#[command(about, version=BUILD_INFO.version, styles=clap_v3_styles())]
enum Args {
    Bidiff(orb_bidiff_squashfs_cli::Args),
}
impl Args {
    async fn run(self, cancel: CancellationToken) -> Result<()> {
        match self {
            Self::Bidiff(cmd) => cmd.run(cancel).await,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let telemetry_flusher = orb_telemetry::TelemetryConfig::new().init();

    let cancel = CancellationToken::new();
    let result = args.run(cancel).await;

    telemetry_flusher.flush().await;

    result
}
