use std::path::PathBuf;

use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use color_eyre::{eyre::Context, Result};
use orb_se050::ExtraData;
use owo_colors::OwoColorize;
use tracing::{debug, info};

#[derive(Debug, Parser)]
#[clap(
    author,
    about,
    version,
    styles = clap_v3_styles(),
)]
struct Args {
    #[arg(long)]
    data: PathBuf,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let _ = orb_telemetry::TelemetryConfig::new().init();

    let args = Args::parse();
    info!("hello world");

    let data = std::fs::read(args.data).wrap_err("failed to read extradata file")?;
    debug!("read {} bytes", data.len());

    let data: ExtraData = data
        .as_slice()
        .try_into()
        .wrap_err("failed to parse ExtraData")?;

    println!(
        "{} {:?}",
        "object attributes:".bold().green(),
        data.object_attributes
    );
    println!("{} {:02X?}", "timestamp:".bold().green(), data.timestamp);
    println!("{} {:02X?}", "freshness:".bold().green(), data.freshness);
    println!("{} {:02X?}", "chip_id:".bold().green(), data.chip_id);

    Ok(())
}

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}
