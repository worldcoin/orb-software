use crate::args::Args;
use color_eyre::{eyre::Context, Result};
use orb_info::{
    orb_os_release::{OrbOsPlatform, OrbOsRelease},
    OrbId,
};
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

#[derive(Debug, Clone)]
pub struct Settings {
    pub orb_id: OrbId,
    pub orb_platform: OrbOsPlatform,
    /// Filesystem path used to persist data
    pub store_path: PathBuf,
    /// Path to the calibration file (configurable for testing)
    pub calibration_file_path: PathBuf,
    /// Path to the OS release file (configurable for testing)
    pub os_release_path: PathBuf,
    /// Path to the versions file (configurable for testing)
    pub versions_file_path: PathBuf,
    /// Path to the downloads directory (configurable for testing)
    pub downloads_path: PathBuf,
    /// Path to the orb name file (configurable for testing)
    pub orb_name_path: PathBuf,
    pub zenoh_port: u16,
}

impl Settings {
    pub async fn from_args(args: &Args, store_path: impl AsRef<Path>) -> Result<Self> {
        let orb_id = if let Some(id) = &args.orb_id {
            OrbId::from_str(id)?
        } else {
            OrbId::read().await?
        };

        let orb_platform = if let Some(platform) = &args.orb_platform {
            match platform.as_str() {
                "diamond" => OrbOsPlatform::Diamond,
                "pearl" => OrbOsPlatform::Pearl,
                _ => unreachable!("handled in argument parsing"),
            }
        } else {
            let os_release = OrbOsRelease::read().await.context(
                "failed to read os-release. Please provide --orb-platform argument",
            )?;
            os_release.orb_os_platform_type
        };

        let downloads_path = match orb_platform {
            OrbOsPlatform::Diamond => PathBuf::from("/mnt/scratch"),
            OrbOsPlatform::Pearl => PathBuf::from("/mnt/updates"),
        };

        Ok(Self {
            orb_id,
            orb_platform,
            store_path: store_path.as_ref().to_path_buf(),
            calibration_file_path: PathBuf::from("/usr/persistent/calibration.json"),
            os_release_path: PathBuf::from("/etc/os-release"),
            versions_file_path: PathBuf::from("/usr/persistent/versions.json"),
            downloads_path,
            orb_name_path: PathBuf::from("/usr/persistent/orb-name"),
            zenoh_port: 7447,
        })
    }
}
