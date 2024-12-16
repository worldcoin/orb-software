use clap::Parser;
use color_eyre::Result;
use orb_build_info::{make_build_info, BuildInfo};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

const BUILD_INFO: BuildInfo = make_build_info!();

// No need to waste RAM with a threadpool.
#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let _args = Cli::parse();

    artificer::run().await
}

#[derive(Parser, Debug)]
#[command(about, author, version=BUILD_INFO.version, styles=make_clap_v3_styles())]
struct Cli {}

/// Colors the CLI help
fn make_clap_v3_styles() -> clap::builder::Styles {
    use clap::builder::styling::AnsiColor;
    clap::builder::Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}
