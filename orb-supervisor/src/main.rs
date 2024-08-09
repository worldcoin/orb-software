use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use color_eyre::eyre::WrapErr as _;
use orb_supervisor::{
    startup::{Application, Settings},
    telemetry::{self, ExecContext},
};
use tracing::debug;
use tracing_subscriber::filter::LevelFilter;

use orb_supervisor::BUILD_INFO;

/// Utility args
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

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    telemetry::start::<ExecContext, _>(LevelFilter::INFO, std::io::stdout)
        .wrap_err("failed to initialize tracing; bailing")?;
    debug!("initialized telemetry");

    let _args = Cli::parse();

    let settings = Settings::default();
    debug!(?settings, "starting supervisor with settings");
    let application = Application::build(settings.clone())
        .await
        .wrap_err("failed to build supervisor")?;

    application.run().await?;

    Ok(())
}
