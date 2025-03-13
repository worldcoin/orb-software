use std::path::Path;
use std::{fs, io::Read};

use clap::Parser;
use eyre::{Result, eyre};
use serde_json::to_string_pretty;

use tracing::{debug, warn};
use tracing_subscriber::{prelude::*, EnvFilter};
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
    let cli = Cli::parse();
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    // Set backend environment variable if specified in command line
    if !cli.backend.is_empty() {
        std::env::set_var("ORB_BACKEND", &cli.backend);
        println!("Using backend environment: {}", cli.backend);
    } else if std::env::var("ORB_BACKEND").is_ok() {
        println!("Using backend environment from ORB_BACKEND: {}", std::env::var("ORB_BACKEND").unwrap_or_default());
    } else {
        println!("Warning: ORB_BACKEND not set. Set using --backend or environment variable.");
    }

    // Set up authentication token - matching fleet-cmdr approach exactly
    let mut _token_task: Option<TokenTaskHandle> = None;
    let cancel_token = CancellationToken::new();
    
    if !cli.disable_auth {
        // Get token using the exact same approach as fleet-cmdr
        let auth_token = if let Some(token) = cli.orb_token.clone() {
            println!("Using token provided via command line or environment");
            let (_, receiver) = tokio::sync::watch::channel(token);
            receiver
        } else if !cli.token_file.is_empty() {
            // Token from specified file
            println!("Using token file: {}", cli.token_file);
            match read_token_from_file(&cli.token_file) {
                Ok(token) => {
                    println!("Successfully read token from file");
                    let (_, receiver) = tokio::sync::watch::channel(token);
                    receiver
                },
                Err(e) => {
                    println!("Warning: Could not read token from file {}: {}", cli.token_file, e);
                    println!("Will attempt to use D-Bus token service...");
                    // Try to use D-Bus token service as in fleet-cmdr (using session bus)
                    match Connection::session().await {
                        Ok(connection) => {
                            match TokenTaskHandle::spawn(&connection, &cancel_token).await {
                                Ok(task) => {
                                    println!("Successfully connected to token service via session bus");
                                    _token_task = Some(task);
                                    _token_task.as_ref().unwrap().token_recv.clone()
                                },
                                Err(e) => {
                                    println!("Error connecting to D-Bus token service: {}", e);
                                    try_token_file_fallback().await
                                }
                            }
                        },
                        Err(e) => {
                            println!("Error connecting to D-Bus session bus: {}", e);
                            try_token_file_fallback().await
                        }
                    }
                }
            }
        } else {
            // Try D-Bus token service via session bus (like fleet-cmdr)
            println!("Setting up authentication via D-Bus token service (session bus)");
            match Connection::session().await {
                Ok(connection) => {
                    match TokenTaskHandle::spawn(&connection, &cancel_token).await {
                        Ok(task) => {
                            println!("Successfully connected to token service via session bus");
                            _token_task = Some(task);
                            _token_task.as_ref().unwrap().token_recv.clone()
                        },
                        Err(e) => {
                            println!("Notice: D-Bus token service unavailable via session bus: {}", e);
                            try_token_file_fallback().await
                        }
                    }
                },
                Err(e) => {
                    println!("Notice: Failed to connect to D-Bus session bus: {}", e);
                    try_token_file_fallback().await
                }
            }
        };
        
        // Set the token receiver for use by the backend status module
        set_token_receiver(auth_token);
    } else {
        println!("Authentication disabled via command-line flag");
    }

    // Always perform WiFi scanning (using iw by default)
    let wifi_networks = if cli.use_wpa {
        println!("Initializing WPA supplicant on {}", cli.wpa_ctrl_path);
        let mut wpa = WpaSupplicant::new(Path::new(&cli.wpa_ctrl_path), !cli.no_mac_filter)?;

        println!("Scanning WiFi networks (performing {} scans)", cli.scan_count);
        
        if cli.include_current_network {
            // Use comprehensive scan including current network
            wpa.comprehensive_scan(cli.scan_count)?
        } else {
            // Use regular scan without specifically adding current network
            wpa.scan_wifi_with_count(cli.scan_count)?
        }
    } else {
        println!("Using iw to scan WiFi networks on interface {}", cli.interface);
        let scanner = IwScanner::new(&cli.interface, !cli.no_mac_filter);
        
        if cli.include_current_network {
            scanner.comprehensive_scan()?
        } else {
            scanner.scan_wifi()?
        }
    };

    println!("WiFi networks found: {}", wifi_networks.len());
    
    // Optional cell modem scanning
    let cellular_info = if cli.enable_cell {
        println!("Scanning cellular networks using device {}", cli.cell_device);
        match scan_cellular(&cli.cell_device) {
            Ok(info) => {
                println!("Cellular information retrieved successfully");
                Some(info)
            }
            Err(e) => {
                warn!("Failed to retrieve cellular information: {}", e);
                None
            }
        }
    } else {
        println!("Cell modem scanning disabled");
        None
    };
    
    // Print the results
    println!("WiFi Networks Found:");
    println!("{}", to_string_pretty(&wifi_networks)?);
    
    if let Some(cell_info) = &cellular_info {
        println!("\nCellular Information:");
        println!("{}", to_string_pretty(cell_info)?);
    }
    
    // Send status update if requested
    if cli.send_status {
        println!("\n======= INITIATING STATUS UPDATE =======");
        println!("Sending status update to backend...");
        
        match send_status_update(&wifi_networks, cellular_info.as_ref()).await {
            Ok(_) => {
                println!("Status update process completed successfully");
            },
            Err(e) => {
                println!("!!! STATUS UPDATE FAILED !!!");
                println!("Error: {}", e);
                warn!("Failed to send status update: {}", e);
            },
        }
        println!("======= STATUS UPDATE COMPLETE =======\n");
    } else {
        println!("\nStatus update sending is disabled");
    }

    // Clean up token task if running
    if _token_task.is_some() {
        println!("Shutting down token service connection");
        cancel_token.cancel();
    }

    Ok(())
}

/// Try to read token from the default file path as fallback
async fn try_token_file_fallback() -> tokio::sync::watch::Receiver<String> {
    // Try default token file fallback
    println!("Trying fallback token file: {}", DEFAULT_TOKEN_FILE_PATH);
    match read_token_from_file(DEFAULT_TOKEN_FILE_PATH) {
        Ok(token) => {
            println!("Successfully read token from default file");
            let (_, receiver) = tokio::sync::watch::channel(token);
            receiver
        },
        Err(e) => {
            println!("Warning: Could not read token from default file: {}", e);
            println!("Authentication will not be available.");
            println!("If you need authentication, use --token-file to specify a valid token file path.");
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
    println!("======= STATUS UPDATE PROCESS =======");
    
    // Check if ORB_BACKEND is set
    if std::env::var("ORB_BACKEND").is_err() {
        println!("ERROR: ORB_BACKEND environment variable is not set!");
        println!("Please set ORB_BACKEND environment variable (e.g., export ORB_BACKEND=stage)");
        println!("Or use the --backend command-line argument.");
        return Err(eyre!("ORB_BACKEND environment variable not set. Use --backend or set environment variable."));
    }
    
    // Get orb ID using the built-in method
    let orb_id = match OrbId::read_blocking() {
        Ok(id) => {
            println!("Successfully retrieved Orb ID: {}", id);
            id
        },
        Err(e) => {
            println!("ERROR: Failed to get Orb ID: {}", e);
            return Err(eyre!("Failed to get Orb ID: {}", e));
        }
    };
    
    println!("Sending data to backend...");
    
    // Send the status update with optional cellular info
    let response_result = orb_location::backend::status::send_location_data(&orb_id, cellular_info, wifi_networks).await;
    
    match response_result {
        Ok(response) => {
            println!("\n======= STATUS UPDATE RESPONSE =======");
            if response.is_empty() {
                println!("Empty response (likely status code 204 No Content)");
            } else {
                println!("Raw response: {}", response);
                println!("\nFormatted JSON (if applicable):");
                
                // Try to pretty print JSON if possible
                match serde_json::from_str::<serde_json::Value>(&response) {
                    Ok(json_value) => println!("{}", serde_json::to_string_pretty(&json_value)?),
                    Err(e) => println!("Response is not valid JSON: {}", e)
                }
            }
            println!("======= END RESPONSE =======\n");
            Ok(())
        },
        Err(e) => {
            println!("\n======= STATUS UPDATE ERROR =======");
            println!("Error sending status update: {}", e);
            
            // Check for common auth errors and provide helpful messages
            let error_str = e.to_string().to_lowercase();
            if error_str.contains("authentication failed") || error_str.contains("unauthorized") {
                println!("\nTroubleshooting authentication:");
                println!("1. Check that the D-Bus token service is running on the session bus");
                println!("2. Verify the token has not expired");
                println!("3. Ensure the Orb ID matches the token");
                println!("4. Try specifying a token file with --token-file=/path/to/token");
                println!("5. Try specifying the token directly with --orb-token=<token>");
            }
            
            println!("Error details: {:?}", e);
            println!("======= END ERROR =======\n");
            Err(eyre!("Failed to send status update: {}", e))
        }
    }
} 
