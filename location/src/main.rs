use std::path::Path;
use std::time::Duration;
use std::{fs, io::Read};

use clap::Parser;
use color_eyre::eyre::{eyre, Result};
use orb_build_info::{make_build_info, BuildInfo};
use orb_location::data::NetworkInfo;
use orb_location::dbus::BackendStatus;
use serde_json::to_string_pretty;

use tokio::signal::unix::{signal, SignalKind};
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, warn};

use orb_location::{
    data::WifiNetwork,
    network_manager::NetworkManager,
    wifi::{IwScanner, WpaSupplicant},
};
use zbus::Connection;

const BUILD_INFO: BuildInfo = make_build_info!();
const SYSLOG_IDENTIFIER: &str = "worldcoin-orb-location";

#[derive(Parser)]
#[command(author, version = BUILD_INFO.version, about)]
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
        // TODO: make this not shitty
        unsafe {
            std::env::set_var("ORB_BACKEND", &cli.backend);
        }
        info!(
            backend = cli.backend,
            "Using backend environment from command line"
        );
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

    // Create the network manager
    let network_manager = NetworkManager::new(cli.network_expiry);

    // Create the backend status client
    let connection = Connection::session().await?;
    let backend_status = BackendStatus::new(&connection).await?;

    // If run-once flag is set, just run once and exit
    if cli.run_once {
        info!("Running in one-time mode");
        let wifi_networks = scan_wifi_networks(&cli).await?;

        // Print the results for debugging
        debug!("WiFi Networks Found: {}", to_string_pretty(&wifi_networks)?);

        if cli.send_status {
            send_status_update_with_retry(
                &wifi_networks,
                &backend_status,
                cli.max_retries,
                cli.operation_timeout,
            )
            .await?;
        }

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
    let mut cleanup_interval =
        time::interval(Duration::from_secs(cli.network_expiry / 2));

    // Main event loop
    tokio::select! {
        _ = async {
            loop {
                tokio::select! {
                    _ = scan_interval.tick() => {
                        scan_and_update_networks(&cli, &network_manager, cli.max_retries).await;
                    }
                    _ = report_interval.tick() => {
                        if cli.send_status {
                            send_periodic_update(&cli, &network_manager, &backend_status).await;
                        }
                    }
                    _ = cleanup_interval.tick() => {
                        network_manager.cleanup_expired().await;
                    }
                }
            }
        } => {},
        _ = cancel_token.cancelled() => {
            info!("Shutdown signal received, stopping main loop");
        }
    }

    info!("Performing cleanup before exit");

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

/// Scan networks and update the network manager
async fn scan_and_update_networks(
    cli: &Cli,
    network_manager: &NetworkManager,
    max_retries: u32,
) {
    for attempt in 0..=max_retries {
        if attempt > 0 {
            debug!(attempt = attempt + 1, "Retrying WiFi scan");
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        match scan_wifi_networks(cli).await {
            Ok(networks) => {
                let new_count = network_manager.update_networks(networks.clone()).await;
                info!(
                    found = networks.len(),
                    new = new_count,
                    total = network_manager.network_count().await,
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
    backend_status: &BackendStatus,
) {
    // Get current networks for reporting
    let networks = network_manager.get_current_networks().await;

    info!(networks = networks.len(), "Sending status update");

    // Send status update with retry
    match send_status_update_with_retry(
        &networks,
        backend_status,
        cli.max_retries,
        cli.operation_timeout,
    )
    .await
    {
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
            let mut wpa =
                WpaSupplicant::new(Path::new(&wpa_ctrl_path), !no_mac_filter)?;

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
    })
    .await?
}

// Send status update with retry logic and timeout
#[instrument(
    skip(wifi_networks, backend_status),
    fields(network_count = wifi_networks.len())
)]
async fn send_status_update_with_retry(
    wifi_networks: &[WifiNetwork],
    backend_status: &BackendStatus,
    max_retries: u32,
    timeout_seconds: u64,
) -> Result<()> {
    for attempt in 0..=max_retries {
        if attempt > 0 {
            debug!(attempt = attempt + 1, "Retrying status update");
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        debug!("Sending status update (attempt {})", attempt + 1);

        let timeout = Duration::from_secs(timeout_seconds);
        match tokio::time::timeout(
            timeout,
            backend_status.send_location_data(&NetworkInfo {
                wifi: wifi_networks.to_vec(),
            }),
        )
        .await
        {
            Ok(Ok(_)) => {
                debug!("Status update successful");
                return Ok(());
            }
            Ok(Err(e)) => {
                if attempt == max_retries {
                    return Err(eyre!(
                        "Status update failed after {} attempts: {}",
                        max_retries + 1,
                        e
                    ));
                } else {
                    debug!(error = %e, "Status update failed, will retry");
                }
            }
            Err(_) => {
                if attempt == max_retries {
                    return Err(eyre!(
                        "Status update timed out after {} seconds (attempt {}/{})",
                        timeout_seconds,
                        attempt + 1,
                        max_retries + 1
                    ));
                } else {
                    debug!(
                        "Status update timed out after {} seconds, will retry",
                        timeout_seconds
                    );
                }
            }
        }
    }

    // This shouldn't be reached due to the above return statements
    Err(eyre!("Unexpected end of retry loop"))
}
