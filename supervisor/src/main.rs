use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use color_eyre::eyre::WrapErr as _;
use orb_supervisor::startup::{Application, Settings};
use std::env;
use tracing::debug;

use orb_supervisor::BUILD_INFO;

const SYSLOG_IDENTIFIER: &str = "worldcoin-supervisor";

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

    let otel_config = orb_telemetry::OpenTelemetryConfig::new(
        "http://localhost:4317",
        SYSLOG_IDENTIFIER,
        BUILD_INFO.version,
        env::var("ORB_BACKEND")
            .expect("ORB_BACKEND environment variable must be set")
            .to_lowercase(),
    );

    let _telemetry_guard = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .with_opentelemetry(otel_config)
        .init();

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
