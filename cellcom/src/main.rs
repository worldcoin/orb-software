use std::path::Path;

use clap::Parser;
use eyre::Result;
use serde_json::to_string_pretty;

use tracing::{debug, info};
use tracing_subscriber::{prelude::*, EnvFilter};

use orb_cellcom::{
    cell::EC25Modem,
    data::{CellularInfo, NetworkInfo},
    geoloc::get_location,
    wifi::WpaSupplicant,
};

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[arg(
        short = 'm',
        long = "modem",
        default_value = "/dev/ttyUSB2",
        help = "Path to the EC25 modem device"
    )]
    modem: String,

    #[arg(
        short = 'w',
        long = "wpa",
        default_value = "/var/run/wpa_supplicant/wlan0",
        help = "Path to the wpa_supplicant control socket"
    )]
    wpa_ctrl_path: String,

    #[arg(long = "no-mac-filter", help = "Disable WiFi MAC address filtering")]
    no_mac_filter: bool,

    #[arg(
        long = "api-key",
        env = "CELLCOM_API_KEY",
        help = "API key for geolocation service. Can also be set via CELLCOM_API_KEY environment variable",
        required = true
    )]
    api_key: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    info!("Initializing modem on {}", cli.modem);
    let mut modem = EC25Modem::new(&cli.modem)?;

    info!("Initializing WPA supplicant on {}", cli.wpa_ctrl_path);
    let mut wpa =
        WpaSupplicant::new(Path::new(&cli.wpa_ctrl_path), !cli.no_mac_filter)?;

    info!(stage = "serving", "Fetching cellular information");
    let serving_cell = modem.get_serving_cell()?;
    info!(stage = "neighbor", "Fetching cellular information");
    let neighbor_cells = modem.get_neighbor_cells()?;
    info!("Scanning WiFi networks");
    let wifi_info = wpa.scan_wifi()?;

    let network_info = NetworkInfo {
        wifi: wifi_info,
        cellular: CellularInfo {
            serving_cell,
            neighbor_cells,
        },
    };

    debug!(?network_info, "Network info collected");

    let location =
        get_location(&cli.api_key, &network_info.cellular, &network_info.wifi)?;

    println!("{}", to_string_pretty(&location)?);

    Ok(())
}
