use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

use crate::BUILD_INFO;

#[derive(Debug, Parser, Serialize, Deserialize)]
#[clap(
    version = BUILD_INFO.version,
    about,
    styles = clap_v3_styles(),
)]
#[skip_serializing_none]
pub struct Args {
    /// The path to the config file.
    #[clap(long)]
    pub config: Option<String>,
    /// The orb id (optional - will be automatically read from system if not provided).
    #[clap(long)]
    pub orb_id: Option<String>,
    /// Orb platform
    #[clap(long, value_parser = ["diamond", "pearl"])]
    pub orb_platform: Option<String>,
    /// The orb token.
    #[clap(long, env = "ORB_TOKEN", default_value = None)]
    pub orb_token: Option<String>,
    /// The relay host.
    #[clap(long, env = "RELAY_HOST", default_value = None)]
    pub relay_host: Option<String>,
    /// The relay namespace.
    #[clap(long, env = "RELAY_NAMESPACE", default_value = "jobs")]
    pub relay_namespace: Option<String>,
    /// The target job-server service id to send messages to.
    #[clap(long, env = "TARGET_SERVICE_ID", default_value = "job-server")]
    pub target_service_id: Option<String>,
    /// D-Bus address (defaults to DBUS_SESSION_BUS_ADDRESS or unix:path=/tmp/worldcoin_bus_socket).
    #[clap(
        long,
        env = "DBUS_SESSION_BUS_ADDRESS",
        default_value = "unix:path=/tmp/worldcoin_bus_socket"
    )]
    pub dbus_addr: String,
    /// Run a single job document locally instead of connecting to relay.
    #[clap(long)]
    pub run_job: Option<String>,
}

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}
