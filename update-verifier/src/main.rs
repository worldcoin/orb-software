mod telemetry;

use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use orb_update_verifier::BUILD_INFO;
use tracing::{error, metadata::LevelFilter};

use crate::telemetry::ExecContext;

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

fn main() {
    if let Err(error) =
        telemetry::start::<ExecContext, _>(LevelFilter::INFO, std::io::stdout)
    {
        error!(
            ?error,
            "update verifier encountered error while starting telemetry"
        );
        std::process::exit(1);
    }

    let _args = Cli::parse();

    if let Err(error) = orb_update_verifier::run_health_check() {
        error!(
            ?error,
            "update verifier encountered error while checking system health"
        );
        std::process::exit(1);
    }
    std::process::exit(0);
}
