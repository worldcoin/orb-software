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
    /// The orb id.
    #[clap(long, env = "ORB_ID", default_value = None)]
    pub orb_id: Option<String>,
    /// The orb token.
    #[clap(long, env = "ORB_TOKEN", default_value = None)]
    pub orb_token: Option<String>,
    /// The relay host.
    #[clap(long, env = "RELAY_HOST", default_value = None)]
    pub relay_host: Option<String>,
    /// The relay namespace.
    #[clap(long, env = "RELAY_NAMESPACE", default_value = "fleet-cmdr")]
    pub relay_namespace: Option<String>,
    /// The fleet-cmdr backend id.
    #[clap(long, env = "FLEET_CMDR_ID", default_value = "fleet-cmdr")]
    pub fleet_cmdr_id: Option<String>,
}

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}
