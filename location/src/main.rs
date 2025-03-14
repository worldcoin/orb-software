use std::path::Path;
use std::{fs, io::Read};

use clap::Parser;
use color_eyre::eyre::{Result, eyre};
use serde_json::to_string_pretty;

use tracing::{debug, info, warn, error};
use tokio_util::sync::CancellationToken;
use zbus::Connection;

use orb_location::{
    wifi::{WpaSupplicant, IwScanner},
    cell::EC25Modem,
    data::{WifiNetwork, CellularInfo},
    backend::status::set_token_receiver,
};
use orb_info::{OrbId, TokenTaskHandle};

// Default token file path (used as fallback when D-Bus service is unavailable)
const DEFAULT_TOKEN_FILE_PATH: &str = "/usr/persistent/token";
const SYSLOG_IDENTIFIER: &str = "worldcoin-orb-location";

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[arg(
        short = 'w',
        long = "wpa",
        default_value = "/var/run/wpa_supplicant/wlan0",
        help = "Path to the wpa_supplicant control socket"
    )]
    wpa_ctrl_path: String,

    #[arg(
        short = 'i',
        long = "interface",
        default_value = "wlan0",
        help = "WiFi interface name"
    )]
    interface: String,

    #[arg(long = "no-mac-filter", help = "Disable WiFi MAC address filtering")]
    no_mac_filter: bool,

    #[arg(
        short = 'c',
        long = "scan-count",
        default_value = "2",
        help = "Number of scans to perform (higher number may yield more complete results)"
    )]
    scan_count: u32,

    #[arg(
        long = "include-current-network",
        help = "Try to include the currently connected WiFi network",
        default_value = "true"
    )]
    include_current_network: bool,

    #[arg(
        long = "use-wpa",
        help = "Use wpa_supplicant instead of iw for scanning",
        default_value = "false"
    )]
    use_wpa: bool,
    
    #[arg(
        long = "enable-cell",
        help = "Enable cell modem scanning",
        default_value = "false"
    )]
    enable_cell: bool,
    
    #[arg(
        long = "cell-device",
        default_value = "/dev/ttyUSB2",
        help = "Path to the cell modem device"
    )]
    cell_device: String,
    
    #[arg(
        long = "send-status",
        help = "Send status update to the backend",
        default_value = "true"
    )]
    send_status: bool,
    
    #[arg(
        long = "backend",
        help = "Backend environment to use (stage, prod, dev). Overrides ORB_BACKEND env var.",
        default_value = ""
    )]
    backend: String,
    
    #[arg(
        long = "disable-auth",
        help = "Disable authentication for backend requests",
        default_value = "false"
    )]
    disable_auth: bool,
    
    #[arg(
        long = "token-file",
        help = "Path to a file containing authentication token (preferred over direct token input for security)",
        default_value = ""
    )]
    token_file: String,

    /// The orb token.
    #[arg(long = "orb-token", env = "ORB_TOKEN", default_value = None)]
    orb_token: Option<String>,
}

/// Helper function to read a token from a file
fn read_token_from_file(path: &str) -> Result<String> {
    debug!("Attempting to read auth token from {}", path);
    let mut file = fs::File::open(path)?;
    let mut token = String::new();
    file.read_to_string(&mut token)?;
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(eyre!("Auth token file exists but is empty"));
    }
    debug!("Successfully read auth token from {}", path);
    Ok(token)
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let tel_flusher = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let cli = Cli::parse();

    // Set backend environment variable if specified in command line
    if !cli.backend.is_empty() {
        std::env::set_var("ORB_BACKEND", &cli.backend);
        info!("Using backend environment: {}", cli.backend);
    } else if std::env::var("ORB_BACKEND").is_ok() {
        info!("Using backend environment from ORB_BACKEND: {}", std::env::var("ORB_BACKEND").unwrap_or_default());
    } else {
        warn!("ORB_BACKEND not set. Set using --backend or environment variable.");
    }

    // Set up authentication token - matching fleet-cmdr approach exactly
    let mut _token_task: Option<TokenTaskHandle> = None;
    let cancel_token = CancellationToken::new();
    
    if !cli.disable_auth {
        // Get token using the exact same approach as fleet-cmdr
        let auth_token = if let Some(token) = cli.orb_token.clone() {
            info!("Using token provided via command line or environment");
            let (_, receiver) = tokio::sync::watch::channel(token);
            receiver
        } else if !cli.token_file.is_empty() {
            // Token from specified file
            info!("Using token file: {}", cli.token_file);
            match read_token_from_file(&cli.token_file) {
                Ok(token) => {
                    info!("Successfully read token from file");
                    let (_, receiver) = tokio::sync::watch::channel(token);
                    receiver
                },
                Err(e) => {
                    warn!("Could not read token from file {}: {}", cli.token_file, e);
                    info!("Will attempt to use D-Bus token service...");
                    // Try to use D-Bus token service as in fleet-cmdr (using session bus)
                    match Connection::session().await {
                        Ok(connection) => {
                            match TokenTaskHandle::spawn(&connection, &cancel_token).await {
                                Ok(task) => {
                                    info!("Successfully connected to token service via session bus");
                                    _token_task = Some(task);
                                    _token_task.as_ref().unwrap().token_recv.clone()
                                },
                                Err(e) => {
                                    error!("Error connecting to D-Bus token service: {}", e);
                                    try_token_file_fallback().await
                                }
                            }
                        },
                        Err(e) => {
                            error!("Error connecting to D-Bus session bus: {}", e);
                            try_token_file_fallback().await
                        }
                    }
                }
            }
        } else {
            // Try D-Bus token service via session bus (like fleet-cmdr)
            info!("Setting up authentication via D-Bus token service (session bus)");
            match Connection::session().await {
                Ok(connection) => {
                    match TokenTaskHandle::spawn(&connection, &cancel_token).await {
                        Ok(task) => {
                            info!("Successfully connected to token service via session bus");
                            _token_task = Some(task);
                            _token_task.as_ref().unwrap().token_recv.clone()
                        },
                        Err(e) => {
                            info!("Notice: D-Bus token service unavailable via session bus: {}", e);
                            try_token_file_fallback().await
                        }
                    }
                },
                Err(e) => {
                    info!("Notice: Failed to connect to D-Bus session bus: {}", e);
                    try_token_file_fallback().await
                }
            }
        };
        
        // Set the token receiver for use by the backend status module
        set_token_receiver(auth_token);
    } else {
        info!("Authentication disabled via command-line flag");
    }

    // Always perform WiFi scanning (using iw by default)
    let wifi_networks = if cli.use_wpa {
        info!("Initializing WPA supplicant on {}", cli.wpa_ctrl_path);
        let mut wpa = WpaSupplicant::new(Path::new(&cli.wpa_ctrl_path), !cli.no_mac_filter)?;

        info!("Scanning WiFi networks (performing {} scans)", cli.scan_count);
        
        if cli.include_current_network {
            // Use comprehensive scan including current network
            wpa.comprehensive_scan(cli.scan_count)?
        } else {
            // Use regular scan without specifically adding current network
            wpa.scan_wifi_with_count(cli.scan_count)?
        }
    } else {
        info!("Using iw to scan WiFi networks on interface {}", cli.interface);
        let scanner = IwScanner::new(&cli.interface, !cli.no_mac_filter);
        
        if cli.include_current_network {
            scanner.comprehensive_scan()?
        } else {
            scanner.scan_wifi()?
        }
    };

    info!("WiFi networks found: {}", wifi_networks.len());
    
    // Optional cell modem scanning
    let cellular_info = if cli.enable_cell {
        info!("Scanning cellular networks using device {}", cli.cell_device);
        match scan_cellular(&cli.cell_device) {
            Ok(info) => {
                info!("Cellular information retrieved successfully");
                Some(info)
            }
            Err(e) => {
                warn!("Failed to retrieve cellular information: {}", e);
                None
            }
        }
    } else {
        info!("Cell modem scanning disabled");
        None
    };
    
    // Print the results
    debug!("WiFi Networks Found:");
    debug!("{}", to_string_pretty(&wifi_networks)?);
    
    if let Some(cell_info) = &cellular_info {
        debug!("\nCellular Information:");
        debug!("{}", to_string_pretty(cell_info)?);
    }
    
    // Send status update if requested
    if cli.send_status {
        info!("Initiating status update");
        info!("Sending status update to backend...");
        
        match send_status_update(&wifi_networks, cellular_info.as_ref()).await {
            Ok(_) => {
                info!("Status update process completed successfully");
            },
            Err(e) => {
                error!("Status update failed: {}", e);
            },
        }
        info!("Status update complete");
    } else {
        info!("Status update sending is disabled");
    }

    // Clean up token task if running
    if _token_task.is_some() {
        info!("Shutting down token service connection");
        cancel_token.cancel();
    }

    tel_flusher.flush().await;
    Ok(())
}

/// Try to read token from the default file path as fallback
async fn try_token_file_fallback() -> tokio::sync::watch::Receiver<String> {
    // Try default token file fallback
    info!("Trying fallback token file: {}", DEFAULT_TOKEN_FILE_PATH);
    match read_token_from_file(DEFAULT_TOKEN_FILE_PATH) {
        Ok(token) => {
            info!("Successfully read token from default file");
            let (_, receiver) = tokio::sync::watch::channel(token);
            receiver
        },
        Err(e) => {
            warn!("Could not read token from default file: {}", e);
            warn!("Authentication will not be available.");
            info!("If you need authentication, use --token-file to specify a valid token file path.");
            // Return an empty token as last resort
            let (_, receiver) = tokio::sync::watch::channel(String::new());
            receiver
        }
    }
}

fn scan_cellular(device: &str) -> Result<CellularInfo> {
    let mut modem = EC25Modem::new(device)?;
    
    debug!("Getting serving cell information");
    let serving_cell = modem.get_serving_cell()?;
    
    debug!("Getting neighbor cell information");
    let neighbor_cells = modem.get_neighbor_cells()?;
    
    Ok(CellularInfo {
        serving_cell,
        neighbor_cells,
    })
}

async fn send_status_update(wifi_networks: &[WifiNetwork], cellular_info: Option<&CellularInfo>) -> Result<()> {
    info!("Status update process started");
    
    // Check if ORB_BACKEND is set
    if std::env::var("ORB_BACKEND").is_err() {
        error!("ORB_BACKEND environment variable is not set!");
        info!("Please set ORB_BACKEND environment variable (e.g., export ORB_BACKEND=stage)");
        info!("Or use the --backend command-line argument.");
        return Err(eyre!("ORB_BACKEND environment variable not set. Use --backend or set environment variable."));
    }
    
    // Get orb ID using the built-in method
    let orb_id = match OrbId::read_blocking() {
        Ok(id) => {
            info!("Successfully retrieved Orb ID: {}", id);
            id
        },
        Err(e) => {
            error!("Failed to get Orb ID: {}", e);
            return Err(eyre!("Failed to get Orb ID: {}", e));
        }
    };
    
    info!("Sending data to backend...");
    
    // Send the status update with optional cellular info
    let response_result = orb_location::backend::status::send_location_data(&orb_id, cellular_info, wifi_networks).await;
    
    match response_result {
        Ok(response) => {
            info!("Status update response received");
            if response.is_empty() {
                debug!("Empty response (likely status code 204 No Content)");
            } else {
                debug!("Raw response: {}", response);
                
                // Try to pretty print JSON if possible
                match serde_json::from_str::<serde_json::Value>(&response) {
                    Ok(json_value) => debug!("Formatted JSON response: {}", serde_json::to_string_pretty(&json_value)?),
                    Err(e) => debug!("Response is not valid JSON: {}", e)
                }
            }
            Ok(())
        },
        Err(e) => {
            error!("Error sending status update: {}", e);
            
            // Check for common auth errors and provide helpful messages
            let error_str = e.to_string().to_lowercase();
            if error_str.contains("authentication failed") || error_str.contains("unauthorized") {
                warn!("Authentication failed. Check token validity and Orb ID match.");
                debug!("Troubleshooting suggestions: Check D-Bus token service, verify token, ensure Orb ID matches");
            }
            
            debug!("Error details: {:?}", e);
            Err(eyre!("Failed to send status update: {}", e))
        }
    }
} 
