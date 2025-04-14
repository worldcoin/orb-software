use clap::Parser;
use color_eyre::Result;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
enum Args {
    Diff,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let telemetry_flusher = orb_telemetry::TelemetryConfig::new().init();

    println!("Hello, world!");

    telemetry_flusher.flush().await;

    Ok(())
}
