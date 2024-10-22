use clap::Parser;
use serde::Serialize;

use crate::BUILD_INFO;

/// An update utility to perform OTA updates of the orb.
///
/// Supports updating Gpt-defined partitions, arbitrary block device offsets, and CAN
/// devices which adhere to the worldcoin CAN protobuf definitions.
///
/// This tool is designed to do exactly as instructed, with no training wheels. It
/// should be used with caution and treated with the same care you might treat
/// `sudo rm -rf` if only for the simple fact that its behavior can achieve similar results.
#[derive(Debug, Parser, Serialize)]
#[command(
    author,
    version = BUILD_INFO.version,
)]
pub struct Args {
    /// The path to the config file.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    /// The path to the `versions.json` file
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub versions: Option<String>,
    #[arg(long, value_enum)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_manifest_signature_against: Option<crate::settings::Backend>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clientkey: Option<String>,
    /// The workspace destination.
    #[arg(long, alias = "wd")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// The download destination.
    #[arg(long, alias = "dir")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloads: Option<String>,
    /// The ID of the orb.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The update source, either a https URL or a local path.
    ///
    /// The `update` alias will be deprecated.
    #[arg(long, alias = "update")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_location: Option<String>,
    /// Prevents the agent from connecting to DBus and communicate with the supervisor.
    ///
    /// The agent will download and install updates without requesting permission from
    /// supervisor.
    #[arg(long)]
    // Serialization is skipped if not set because command line args always take
    // precedence over env vars and a config file. This would otherwise make it
    // impossible to set this config option outside of cli args.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub nodbus: bool,
    /// Skips verification of versions asserted in the claim if the update manifest contains
    /// updated components for all components on the orb.
    #[arg(long)]
    // Serialization is skipped if not set because command line args always take
    // precedence over env vars and a config file. This would otherwise make it
    // impossible to set this config option outside of cli args.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub skip_version_asserts: bool,
    /// Downloads all components, but does not execute the actual update step, copying components
    /// to their destinations, etc.
    #[arg(long)]
    // Serialization is skipped if not set because command line args always take
    // precedence over env vars and a config file. This would otherwise make it
    // impossible to set this config option outside of cli args.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub noupdate: bool,
    /// Puts update agent into recovery mode, allowing it to update components that have
    /// `"installation_phase": "recovery"` set. This flag should only be set when actually
    /// running in recovery!
    #[arg(long)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub recovery: bool,
    /// Duration in milliseconds that the agent will wait between downloading chunks of a
    /// component. If `--nodbus` is specified, then agent will always wait for this amount of time.
    /// If the supervisor dbus service indicated that no downloads occured for more than one hour,
    /// then the agent will skip the wait.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_delay: Option<u64>,
    #[clap(long)]
    pub(super) token: Option<String>,
}
