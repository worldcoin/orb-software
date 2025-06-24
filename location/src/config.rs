use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use serde::{Deserialize, Serialize};

/// Primary application configuration struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// WiFi configuration
    pub wifi: WiFiConfig,

    /// Cellular configuration
    pub cellular: CellularConfig,

    /// Backend configuration
    pub backend: BackendConfig,

    /// Service configuration
    pub service: ServiceConfig,
}

/// WiFi specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WiFiConfig {
    /// Path to the wpa_supplicant control socket
    pub wpa_ctrl_path: PathBuf,

    /// WiFi interface name
    pub interface: String,

    /// Whether to use MAC address filtering
    pub use_mac_filter: bool,

    /// Whether to include the currently connected network in scan results
    pub include_current_network: bool,

    /// Whether to use wpa_supplicant for scanning instead of iw
    pub use_wpa: bool,
}

/// Cellular specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellularConfig {
    /// Whether cellular scanning is enabled
    pub enabled: bool,

    /// Path to the cell modem device
    pub device: PathBuf,
}

/// Backend communication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    /// Whether to send status updates to the backend
    pub send_status: bool,

    /// Backend environment (stage, prod, dev)
    pub environment: Option<String>,

    /// Whether to disable authentication for backend requests
    pub disable_auth: bool,

    /// Path to a file containing authentication token
    pub token_file: Option<PathBuf>,

    /// Direct auth token (less secure)
    pub token: Option<String>,
}

/// General service configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    /// Time interval between WiFi scans in seconds
    pub scan_interval: Duration,

    /// Time in seconds before a network is considered stale
    pub network_expiry: Duration,

    /// Time interval between backend status reports in seconds
    pub report_interval: Duration,

    /// Whether to run once and exit
    pub run_once: bool,

    /// Timeout for network operations
    pub operation_timeout: Duration,

    /// Maximum number of retries for failed operations
    pub max_retries: u32,
}

impl Config {
    /// Create a new config from CLI arguments
    pub fn from_cli(cli: &Cli) -> Self {
        let wifi = WiFiConfig {
            wpa_ctrl_path: PathBuf::from(&cli.wpa_ctrl_path),
            interface: cli.interface.clone(),
            use_mac_filter: !cli.no_mac_filter,
            include_current_network: cli.include_current_network,
            use_wpa: cli.use_wpa,
        };

        let cellular = CellularConfig {
            enabled: cli.enable_cell,
            device: PathBuf::from(&cli.cell_device),
        };

        let backend = BackendConfig {
            send_status: cli.send_status,
            environment: if cli.backend.is_empty() {
                None
            } else {
                Some(cli.backend.clone())
            },
            disable_auth: cli.disable_auth,
            token_file: if cli.token_file.is_empty() {
                None
            } else {
                Some(PathBuf::from(&cli.token_file))
            },
            token: cli.orb_token.clone(),
        };

        let service = ServiceConfig {
            scan_interval: Duration::from_secs(cli.scan_interval),
            network_expiry: Duration::from_secs(cli.network_expiry),
            report_interval: Duration::from_secs(cli.report_interval),
            run_once: cli.run_once,
            operation_timeout: Duration::from_secs(cli.operation_timeout),
            max_retries: cli.max_retries,
        };

        Self {
            wifi,
            cellular,
            backend,
            service,
        }
    }

    // /// Set the environment variables based on the configuration
    // pub fn setup_environment(&self) -> Result<()> {
    //     // Set backend environment variable if specified in config
    //     if let Some(env) = &self.backend.environment {
    //         std::env::set_var("ORB_BACKEND", env);
    //         info!(backend = %env, "Using backend environment from configuration");
    //     } else if std::env::var("ORB_BACKEND").is_ok() {
    //         info!(
    //             backend = %std::env::var("ORB_BACKEND").unwrap_or_default(),
    //             "Using backend environment from ORB_BACKEND env var"
    //         );
    //     } else {
    //         warn!("ORB_BACKEND not set. Backend operations may fail.");
    //         return Err(LocationError::ConfigError(
    //             "ORB_BACKEND environment variable not set. Use the backend option or set environment variable.".into()
    //         ));
    //     }
    //
    //     Ok(())
    // }
}

// CLI parser using clap - matches the original CLI but allows for conversion to Config
#[derive(Parser)]
#[command(author, version, about)]
pub struct Cli {
    #[arg(
        short = 'w',
        long = "wpa",
        default_value = "/var/run/wpa_supplicant/wlan0",
        help = "Path to the wpa_supplicant control socket"
    )]
    pub wpa_ctrl_path: String,

    #[arg(
        short = 'i',
        long = "interface",
        default_value = "wlan0",
        help = "WiFi interface name"
    )]
    pub interface: String,

    #[arg(long = "no-mac-filter", help = "Disable WiFi MAC address filtering")]
    pub no_mac_filter: bool,

    #[arg(
        long = "include-current-network",
        help = "Try to include the currently connected WiFi network",
        default_value = "true"
    )]
    pub include_current_network: bool,

    #[arg(
        long = "use-wpa",
        help = "Use wpa_supplicant instead of iw for scanning",
        default_value = "false"
    )]
    pub use_wpa: bool,

    #[arg(
        long = "enable-cell",
        help = "Enable cell modem scanning",
        default_value = "false"
    )]
    pub enable_cell: bool,

    #[arg(
        long = "cell-device",
        default_value = "/dev/ttyUSB2",
        help = "Path to the cell modem device"
    )]
    pub cell_device: String,

    #[arg(
        long = "send-status",
        help = "Send status update to the backend",
        default_value = "true"
    )]
    pub send_status: bool,

    #[arg(
        long = "backend",
        help = "Backend environment to use (stage, prod, dev). Overrides ORB_BACKEND env var.",
        default_value = ""
    )]
    pub backend: String,

    #[arg(
        long = "disable-auth",
        help = "Disable authentication for backend requests",
        default_value = "false"
    )]
    pub disable_auth: bool,

    #[arg(
        long = "token-file",
        help = "Path to a file containing authentication token (preferred over direct token input for security)",
        default_value = ""
    )]
    pub token_file: String,

    /// The orb token.
    #[arg(long = "orb-token", env = "ORB_TOKEN", default_value = None)]
    pub orb_token: Option<String>,

    #[arg(
        long = "scan-interval",
        help = "Time interval between WiFi scans in seconds",
        default_value = "5"
    )]
    pub scan_interval: u64,

    #[arg(
        long = "network-expiry",
        help = "Time in seconds before a network is considered stale and removed from memory",
        default_value = "60"
    )]
    pub network_expiry: u64,

    #[arg(
        long = "report-interval",
        help = "Time interval between backend status reports in seconds",
        default_value = "10"
    )]
    pub report_interval: u64,

    #[arg(
        long = "run-once",
        help = "Run once and exit (instead of continuous monitoring)",
        default_value = "false"
    )]
    pub run_once: bool,

    #[arg(
        long = "operation-timeout",
        help = "Timeout in seconds for network operations",
        default_value = "30"
    )]
    pub operation_timeout: u64,

    #[arg(
        long = "max-retries",
        help = "Maximum number of retries for failed operations",
        default_value = "3"
    )]
    pub max_retries: u32,
}
