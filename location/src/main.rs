use std::path::Path;
use std::{fs, io::Read};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use color_eyre::eyre::{Result, eyre};
use serde_json::to_string_pretty;

use tracing::{debug, info, warn, error, instrument};
use tokio_util::sync::CancellationToken;
use tokio::time;
use tokio::signal::unix::{signal, SignalKind};
use zbus::Connection;

use orb_location::{
    wifi::{WpaSupplicant, IwScanner},
    cell::EC25Modem,
    data::{WifiNetwork, CellularInfo},
    backend::status::set_token_receiver,
    network_manager::NetworkManager,
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
    
    #[arg(
        long = "scan-interval",
        help = "Time interval between WiFi scans in seconds",
        default_value = "5"
    )]
    scan_interval: u64,
    
    #[arg(
        long = "network-expiry",
        help = "Time in seconds before a network is considered stale and removed from memory",
        default_value = "60"
    )]
    network_expiry: u64,
    
    #[arg(
        long = "report-interval",
        help = "Time interval between backend status reports in seconds",
        default_value = "10"
    )]
    report_interval: u64,
    
    #[arg(
        long = "run-once",
        help = "Run once and exit (instead of continuous monitoring)",
        default_value = "false"
    )]
    run_once: bool,
    
    #[arg(
        long = "operation-timeout",
        help = "Timeout in seconds for network operations",
        default_value = "30"
    )]
    operation_timeout: u64,
    
    #[arg(
        long = "max-retries",
        help = "Maximum number of retries for failed operations",
        default_value = "3"
    )]
    max_retries: u32,
}

/// Helper function to read a token from a file
#[instrument(level = "debug", skip_all, fields(path = %path))]
fn read_token_from_file(path: &str) -> Result<String> {
    debug!("Attempting to read auth token from file");
    let mut file = fs::File::open(path)?;
    let mut token = String::new();
    file.read_to_string(&mut token)?;
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(eyre!("Auth token file exists but is empty"));
    }
    debug!("Successfully read auth token");
    Ok(token)
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let tel_flusher = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let cli = Cli::parse();
    info!(
        interface = cli.interface,
        use_wpa = cli.use_wpa,
        scan_interval = cli.scan_interval,
        report_interval = cli.report_interval,
        network_expiry = cli.network_expiry,
        "Starting Orb Location service"
    );

    // Set backend environment variable if specified in command line
    if !cli.backend.is_empty() {
        std::env::set_var("ORB_BACKEND", &cli.backend);
        info!(backend = cli.backend, "Using backend environment from command line");
    } else if std::env::var("ORB_BACKEND").is_ok() {
        info!(
            backend = %std::env::var("ORB_BACKEND").unwrap_or_default(),
            "Using backend environment from ORB_BACKEND env var"
        );
    } else {
        warn!("ORB_BACKEND not set. Set using --backend or environment variable.");
    }

    // Create a cancellation token for coordinating shutdown
    let cancel_token = CancellationToken::new();
    
    // Set up authentication token - matching fleet-cmdr approach exactly
    let mut _token_task: Option<TokenTaskHandle> = None;
    
    if !cli.disable_auth {
        // Get token using the exact same approach as fleet-cmdr
        let auth_token = if let Some(token) = cli.orb_token.clone() {
            info!("Using token provided via command line or environment");
            let (_, receiver) = tokio::sync::watch::channel(token);
            receiver
        } else if !cli.token_file.is_empty() {
            // Token from specified file
            info!(file = cli.token_file, "Using token file");
            match read_token_from_file(&cli.token_file) {
                Ok(token) => {
                    info!("Successfully read token from file");
                    let (_, receiver) = tokio::sync::watch::channel(token);
                    receiver
                },
                Err(e) => {
                    warn!(error = %e, "Could not read token from file, trying D-Bus");
                    setup_dbus_token(&cancel_token).await
                }
            }
        } else {
            // Try D-Bus token service via session bus (like fleet-cmdr)
            info!("Setting up authentication via D-Bus token service");
            setup_dbus_token(&cancel_token).await
        };
        
        // Set the token receiver for use by the backend status module
        set_token_receiver(auth_token);
    } else {
        info!("Authentication disabled via command-line flag");
    }

    // Set up cellular info storage if enabled
    let cell_info: Arc<Mutex<Option<CellularInfo>>> = Arc::new(Mutex::new(None));
    
    // Create the network manager
    let network_manager = NetworkManager::new(cli.network_expiry);
    
    // If run-once flag is set, just run once and exit
    if cli.run_once {
        info!("Running in one-time mode");
        let (wifi_networks, cellular_info) = perform_scan(&cli, cli.max_retries).await?;
        
        // Update network manager
        let new_count = network_manager.update_networks(wifi_networks.clone());
        info!(count = wifi_networks.len(), new = new_count, "Found WiFi networks");
        
        // Send status update if requested
        if cli.send_status {
            info!("Sending status update to backend");
            match send_status_update_with_retry(&wifi_networks, cellular_info.as_ref(), cli.max_retries, cli.operation_timeout).await {
                Ok(_) => info!("Status update completed successfully"),
                Err(e) => error!(error = %e, "Status update failed"),
            }
        }
        
        // Clean up token task if running
        if _token_task.is_some() {
            info!("Shutting down token service connection");
            cancel_token.cancel();
        }
        
        tel_flusher.flush().await;
        return Ok(());
    }

    // Set up signal handling for graceful shutdown
    setup_signal_handling(cancel_token.clone());
    
    info!(
        scan_interval = cli.scan_interval,
        report_interval = cli.report_interval,
        network_expiry = cli.network_expiry,
        "Starting continuous monitoring"
    );
    
    // Create interval timers
    let mut scan_interval = time::interval(Duration::from_secs(cli.scan_interval));
    let mut report_interval = time::interval(Duration::from_secs(cli.report_interval));
    let mut cleanup_interval = time::interval(Duration::from_secs(cli.network_expiry / 2));
    
    // If cellular scanning is enabled, set up separate task for it
    // (cell scanning tends to be slower and shouldn't block WiFi scanning)
    if cli.enable_cell {
        setup_cellular_scanning(
            &cli, 
            cell_info.clone(), 
            cancel_token.clone()
        );
    }
    
    // Main event loop
    let cell_scanning = cli.enable_cell;
    tokio::select! {
        _ = async {
            loop {
                tokio::select! {
                    _ = scan_interval.tick() => {
                        scan_and_update_networks(&cli, &network_manager, cli.max_retries).await;
                    }
                    _ = report_interval.tick() => {
                        if cli.send_status {
                            send_periodic_update(&cli, &network_manager, cell_info.clone(), cell_scanning).await;
                        }
                    }
                    _ = cleanup_interval.tick() => {
                        network_manager.cleanup_expired();
                    }
                }
            }
        } => {},
        _ = cancel_token.cancelled() => {
            info!("Shutdown signal received, stopping main loop");
        }
    }
    
    info!("Performing cleanup before exit");
    
    // Clean up token task if running
    if _token_task.is_some() {
        info!("Shutting down token service connection");
    }

    tel_flusher.flush().await;
    info!("Exiting gracefully");
    Ok(())
}

/// Sets up signal handling for SIGINT and SIGTERM
fn setup_signal_handling(cancel_token: CancellationToken) {
    tokio::spawn(async move {
        let mut sigint = signal(SignalKind::interrupt()).unwrap();
        let mut sigterm = signal(SignalKind::terminate()).unwrap();
        
        tokio::select! {
            _ = sigint.recv() => {
                info!("Received SIGINT, shutting down gracefully");
            }
            _ = sigterm.recv() => {
                info!("Received SIGTERM, shutting down gracefully");
            }
        }
        
        cancel_token.cancel();
    });
}

/// Sets up D-Bus token service
async fn setup_dbus_token(cancel_token: &CancellationToken) -> tokio::sync::watch::Receiver<String> {
    match Connection::session().await {
        Ok(connection) => {
            match TokenTaskHandle::spawn(&connection, cancel_token).await {
                Ok(task) => {
                    info!("Successfully connected to token service via session bus");
                    task.token_recv.clone()
                },
                Err(e) => {
                    info!(error = %e, "D-Bus token service unavailable via session bus");
                    try_token_file_fallback().await
                }
            }
        },
        Err(e) => {
            info!(error = %e, "Failed to connect to D-Bus session bus");
            try_token_file_fallback().await
        }
    }
}

/// Sets up cellular scanning in a separate task
fn setup_cellular_scanning(
    cli: &Cli, 
    cell_info: Arc<Mutex<Option<CellularInfo>>>, 
    cancel_token: CancellationToken
) {
    info!("Enabling cellular scanning");
    let cell_device = cli.cell_device.clone();
    let cell_info_clone = cell_info.clone();
    let child_token = cancel_token.child_token();
    let scan_interval = cli.scan_interval * 2;
    let max_retries = cli.max_retries;
    
    tokio::spawn(async move {
        let mut cell_interval = time::interval(Duration::from_secs(scan_interval));
        
        loop {
            tokio::select! {
                _ = cell_interval.tick() => {
                    for attempt in 0..=max_retries {
                        if attempt > 0 {
                            debug!(attempt = attempt + 1, "Retrying cellular scan");
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                        
                        match scan_cellular(&cell_device) {
                            Ok(info) => {
                                debug!("Updated cellular information");
                                let mut cell_data = cell_info_clone.lock().unwrap();
                                *cell_data = Some(info);
                                break;
                            }
                            Err(e) => {
                                if attempt == max_retries {
                                    warn!(error = %e, "Failed to retrieve cellular information after multiple attempts");
                                } else {
                                    debug!(error = %e, "Failed to retrieve cellular information, will retry");
                                }
                            }
                        }
                    }
                }
                _ = child_token.cancelled() => {
                    info!("Shutting down cellular scanning");
                    break;
                }
            }
        }
    });
}

/// Scan networks and update the network manager
async fn scan_and_update_networks(cli: &Cli, network_manager: &NetworkManager, max_retries: u32) {
    for attempt in 0..=max_retries {
        if attempt > 0 {
            debug!(attempt = attempt + 1, "Retrying WiFi scan");
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        
        match scan_wifi_networks(cli).await {
            Ok(networks) => {
                let new_count = network_manager.update_networks(networks.clone());
                info!(
                    found = networks.len(), 
                    new = new_count, 
                    total = network_manager.network_count(),
                    "WiFi scan complete"
                );
                break;
            }
            Err(e) => {
                if attempt == max_retries {
                    error!(error = %e, "WiFi scan failed after multiple attempts");
                } else {
                    debug!(error = %e, "WiFi scan failed, will retry");
                }
            }
        }
    }
}

/// Send periodic update to backend
async fn send_periodic_update(
    cli: &Cli,
    network_manager: &NetworkManager,
    cell_info: Arc<Mutex<Option<CellularInfo>>>,
    cell_enabled: bool
) {
    // Get current networks for reporting
    let networks = network_manager.get_current_networks();
    let cell_data = if cell_enabled {
        let guard = cell_info.lock().unwrap();
        guard.as_ref().map(|info| info.clone())
    } else {
        None
    };
    
    info!(networks = networks.len(), "Sending status update");
    
    // Send status update with retry
    match send_status_update_with_retry(&networks, cell_data.as_ref(), cli.max_retries, cli.operation_timeout).await {
        Ok(_) => debug!("Status update completed successfully"),
        Err(e) => error!(error = %e, "Status update failed"),
    }
}

// Helper function to scan WiFi networks with timeout
#[instrument(skip(cli), fields(interface = %cli.interface, use_wpa = cli.use_wpa))]
async fn scan_wifi_networks(cli: &Cli) -> Result<Vec<WifiNetwork>> {
    // Clone the values we need to avoid lifetime issues
    let wpa_ctrl_path = cli.wpa_ctrl_path.clone();
    let interface = cli.interface.clone();
    let use_wpa = cli.use_wpa;
    let no_mac_filter = cli.no_mac_filter;
    let include_current_network = cli.include_current_network;
    
    // We need to spawn scanning in a blocking task because the underlying libraries are not async
    tokio::task::spawn_blocking(move || {
        if use_wpa {
            debug!("Scanning with WPA supplicant");
            let mut wpa = WpaSupplicant::new(Path::new(&wpa_ctrl_path), !no_mac_filter)?;
            
            if include_current_network {
                wpa.comprehensive_scan(1)
            } else {
                wpa.scan_wifi_with_count(1)
            }
        } else {
            debug!("Scanning with iw");
            let scanner = IwScanner::new(&interface, !no_mac_filter);
            
            if include_current_network {
                scanner.comprehensive_scan_with_count(1)
            } else {
                scanner.scan_wifi_with_count(1)
            }
        }
    }).await?
}

// Helper function to perform a one-time scan of WiFi and optionally cellular
#[instrument(skip(cli), fields(interface = %cli.interface, enable_cell = cli.enable_cell))]
async fn perform_scan(cli: &Cli, max_retries: u32) -> Result<(Vec<WifiNetwork>, Option<CellularInfo>)> {
    // Scan WiFi networks with retry
    let mut wifi_networks = Vec::new();
    
    for attempt in 0..=max_retries {
        if attempt > 0 {
            debug!(attempt = attempt + 1, "Retrying WiFi scan");
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        
        match scan_wifi_networks(cli).await {
            Ok(networks) => {
                wifi_networks = networks;
                info!(count = wifi_networks.len(), "WiFi networks found");
                break;
            }
            Err(e) => {
                if attempt == max_retries {
                    return Err(eyre!("Failed to scan WiFi after {} attempts: {}", max_retries, e));
                } else {
                    debug!(error = %e, "WiFi scan failed, will retry");
                }
            }
        }
    }
    
    // Optional cell modem scanning
    let cellular_info = if cli.enable_cell {
        info!(device = cli.cell_device, "Scanning cellular networks");
        
        let mut cell_info = None;
        for attempt in 0..=max_retries {
            if attempt > 0 {
                debug!(attempt = attempt + 1, "Retrying cellular scan");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            
            match scan_cellular(&cli.cell_device) {
                Ok(info) => {
                    info!("Cellular information retrieved successfully");
                    cell_info = Some(info);
                    break;
                }
                Err(e) => {
                    if attempt == max_retries {
                        warn!(error = %e, "Failed to retrieve cellular information after multiple attempts");
                    } else {
                        debug!(error = %e, "Failed to retrieve cellular information, will retry");
                    }
                }
            }
        }
        
        cell_info
    } else {
        info!("Cell modem scanning disabled");
        None
    };
    
    // Print the results for debugging
    debug!("WiFi Networks Found: {}", to_string_pretty(&wifi_networks)?);
    
    if let Some(cell_info) = &cellular_info {
        debug!("Cellular Information: {}", to_string_pretty(cell_info)?);
    }
    
    Ok((wifi_networks, cellular_info))
}

#[instrument(skip_all, fields(device = %device))]
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

// Send status update with retry logic and timeout
#[instrument(skip(wifi_networks, cellular_info), fields(network_count = wifi_networks.len(), has_cell = cellular_info.is_some()))]
async fn send_status_update_with_retry(
    wifi_networks: &[WifiNetwork], 
    cellular_info: Option<&CellularInfo>,
    max_retries: u32,
    timeout_seconds: u64
) -> Result<()> {
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
            info!(orb_id = %id, "Successfully retrieved Orb ID");
            id
        },
        Err(e) => {
            error!(error = %e, "Failed to get Orb ID");
            return Err(eyre!("Failed to get Orb ID: {}", e));
        }
    };
    
    info!("Sending data to backend");
    
    // Send the status update with optional cellular info with retries
    for attempt in 0..=max_retries {
        if attempt > 0 {
            debug!(attempt = attempt + 1, "Retrying backend status update");
            // Add exponential backoff
            tokio::time::sleep(Duration::from_millis(500 * (1 << attempt))).await;
        }
        
        // Set a timeout for the backend request
        match tokio::time::timeout(
            Duration::from_secs(timeout_seconds),
            orb_location::backend::status::send_location_data(&orb_id, cellular_info, wifi_networks)
        ).await {
            Ok(response_result) => match response_result {
                Ok(response) => {
                    info!("Status update response received");
                    if response.is_empty() {
                        debug!("Empty response (likely status code 204 No Content)");
                    } else {
                        debug!(response = %response, "Raw response received");
                        
                        // Try to pretty print JSON if possible
                        match serde_json::from_str::<serde_json::Value>(&response) {
                            Ok(json_value) => debug!(json = %serde_json::to_string_pretty(&json_value)?, "Formatted JSON response"),
                            Err(e) => debug!(error = %e, "Response is not valid JSON")
                        }
                    }
                    return Ok(());
                },
                Err(e) => {
                    let error_str = e.to_string().to_lowercase();
                    if error_str.contains("authentication failed") || error_str.contains("unauthorized") {
                        warn!(error = %e, "Authentication failed. Check token validity and Orb ID match.");
                        debug!("Troubleshooting suggestions: Check D-Bus token service, verify token, ensure Orb ID matches");
                        // Authentication errors likely won't resolve with retries
                        return Err(eyre!("Failed to send status update: {}", e));
                    }
                    
                    if attempt == max_retries {
                        error!(error = %e, "Error sending status update after multiple attempts");
                        return Err(eyre!("Failed to send status update: {}", e));
                    } else {
                        debug!(error = %e, "Error sending status update, will retry");
                    }
                }
            },
            Err(_) => {
                if attempt == max_retries {
                    error!("Backend request timed out after {} seconds", timeout_seconds);
                    return Err(eyre!("Backend request timed out after {} seconds", timeout_seconds));
                } else {
                    debug!("Backend request timed out, will retry");
                }
            }
        }
    }
    
    // Should not reach here due to loop structure but compiler needs this
    Err(eyre!("Failed to send status update after all retries"))
}

/// Try to read token from the default file path as fallback
#[instrument(skip_all)]
async fn try_token_file_fallback() -> tokio::sync::watch::Receiver<String> {
    // Try default token file fallback
    info!(path = DEFAULT_TOKEN_FILE_PATH, "Trying fallback token file");
    match read_token_from_file(DEFAULT_TOKEN_FILE_PATH) {
        Ok(token) => {
            info!("Successfully read token from default file");
            let (_, receiver) = tokio::sync::watch::channel(token);
            receiver
        },
        Err(e) => {
            warn!(error = %e, "Could not read token from default file");
            warn!("Authentication will not be available.");
            info!("If you need authentication, use --token-file to specify a valid token file path.");
            // Return an empty token as last resort
            let (_, receiver) = tokio::sync::watch::channel(String::new());
            receiver
        }
    }
} 
