use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

use crate::BUILD_INFO;

#[derive(Debug, Parser, Serialize, Deserialize, Default)]
#[clap(
    version = BUILD_INFO.version,
    about,
    styles = clap_v3_styles(),
)]
#[skip_serializing_none]
pub struct Args {
    /// The orb id.
    #[clap(long, env = "ORB_ID", default_value = None)]
    pub orb_id: Option<String>,
    /// The orb token.
    #[clap(long, env = "ORB_TOKEN", default_value = None)]
    pub orb_token: Option<String>,
    /// The backend to use.
    #[clap(long, env = "ORB_BACKEND", default_value = "stage")]
    pub backend: String,
    /// status local address
    #[clap(
        long,
        env = "ORB_STATUS_LOCAL_ADDRESS",
        default_value = None
    )]
    pub status_local_address: Option<String>,
    /// status update interval in seconds.
    #[clap(long, env = "ORB_STATUS_UPDATE_INTERVAL", default_value = "30")]
    pub status_update_interval: u64,
}

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}
