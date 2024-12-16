use std::path::PathBuf;

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
    #[arg(long)]
    pub config: Option<String>,
    /// The URL of the orb relay.
    #[arg(long, default_value = "https://relay.worldcoin.org")]
    pub orb_relay_url: Option<String>,
    /// The path to the orb name file.
    #[arg(long, default_value = "/etc/worldcoin/orb_name")]
    pub orb_name_path: Option<PathBuf>,
}

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}
