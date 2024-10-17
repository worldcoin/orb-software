use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use figment::providers::Format as _;
use orb_update_agent_core::{LocalOrRemote, Slot};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DurationMilliSeconds};

mod args;
pub use args::Args;

#[cfg(test)]
mod tests;

/// The different types of backend environments.
#[derive(
    Debug, Eq, PartialEq, Serialize, Deserialize, Copy, Clone, clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Prod,
    Stage,
}

/// `Settings` are the configurable options for running the update agent.
///
/// The only entry point to construct `Settings` is `Settings::get`.
#[serde_as]
#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Settings {
    pub versions: PathBuf,
    /// Pub keys are in [`orb_update_agent_core::pubkeys`]
    pub verify_manifest_signature_against: Backend,
    pub clientkey: PathBuf,
    pub active_slot: Slot,
    pub workspace: PathBuf,
    pub downloads: PathBuf,
    pub id: String,
    pub update_location: LocalOrRemote,
    pub nodbus: bool,
    pub skip_version_asserts: bool,
    pub noupdate: bool,
    pub recovery: bool,
    #[serde_as(as = "DurationMilliSeconds")]
    pub download_delay: Duration,
    pub token: Option<String>,
}

impl Settings {
    /// Constructs `Settings` from a config file, environment variables, and command line
    /// arguments. Command line arguments always take precedence over environment variables, which
    /// in turn take precedence over the config file.
    pub fn get<P: AsRef<Path>>(
        args: &Args,
        config: P,
        env_prefix: &str,
        active_slot: Slot,
    ) -> figment::error::Result<Settings> {
        figment::Figment::new()
            .merge(figment::providers::Serialized::default(
                "active_slot",
                active_slot,
            ))
            .merge(figment::providers::Toml::file(config))
            .merge(figment::providers::Env::prefixed(env_prefix))
            .merge(figment::providers::Serialized::defaults(args))
            .extract()
    }
}
