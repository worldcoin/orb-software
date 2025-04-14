use clap::{Parser, Subcommand};
use color_eyre::Result;

#[derive(Debug, Subcommand)]
#[command(version, about, long_about = None)]
enum Args {
    Diff(orb_bidiff_squashfs_cli::Args),
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    //let args = Args::parse();
    let telemetry_flusher = orb_telemetry::TelemetryConfig::new().init();

    println!("Hello, world!");

    telemetry_flusher.flush().await;

    Ok(())
}
