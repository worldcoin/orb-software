use std::str::FromStr;

use camino::Utf8PathBuf;
use clap::Parser;
use cmd_lib::run_fun;
use color_eyre::{
    eyre::{bail, eyre, OptionExt, WrapErr},
    Result, Section,
};
use orb_s3_helpers::{ExistingFileBehavior, S3Uri};
use thiserror::Error;
use tokio::task::spawn_blocking;
use tracing::{debug, info, warn};

/// Boot the orb using NFS
#[derive(Debug, Parser)]
pub struct Nfsboot {
    /// The s3 URI of the RTS to use.
    #[arg(
        long,
        conflicts_with = "rts_path",
        required_unless_present = "rts_path"
    )]
    s3_url: Option<S3Uri>,
    /// The directory to save the s3 artifact we download.
    #[arg(long)]
    download_dir: Option<Utf8PathBuf>,
    /// Path to a downloaded RTS (zipped .tar or an already-extracted directory).
    #[arg(
        long,
        conflicts_with = "s3_url",
        conflicts_with = "download_dir",
        required_unless_present = "s3_url"
    )]
    rts_path: Option<Utf8PathBuf>,
    /// If this flag is given, overwites any existing files when downloading the rts.
    #[arg(long)]
    overwrite_existing: bool,
    /// Bind-mounts in the form <orb_mount_name>,<host_path>. Repeat --mount to add more.
    #[arg(long = "mount")]
    mounts: Vec<MountSpec>,
}

impl Nfsboot {
    pub async fn run(self) -> Result<()> {
        debug!("nfsboot called with args {self:?}");
        error_detection_for_host_state()
            .await
            .wrap_err("failed to assert host's state")?;
        let rts_path = self.maybe_download_rts().await?;
        debug!("resolved RTS path: {rts_path}");

        todo!()
    }

    async fn maybe_download_rts(&self) -> Result<Utf8PathBuf> {
        let existing_file_behavior = if self.overwrite_existing {
            ExistingFileBehavior::Overwrite
        } else {
            ExistingFileBehavior::Abort
        };
        // Determine RTS tarball path: download from S3 or use provided path
        let rts_path = if let Some(ref s3_url) = self.s3_url {
            assert!(
                self.rts_path.is_none(),
                "sanity: mutual exclusion guaranteed by clap"
            );
            let download_dir =
                self.download_dir.clone().unwrap_or_else(crate::current_dir);
            let download_path = download_dir.join(
                crate::download_s3::parse_filename(s3_url)
                    .wrap_err("failed to parse filename")?,
            );

            crate::download_s3::download_url(
                s3_url,
                &download_path,
                existing_file_behavior,
            )
            .await
            .wrap_err("error while downloading from s3")?;

            download_path
        } else if let Some(rts_path) = self.rts_path.clone() {
            assert!(
                self.s3_url.is_none(),
                "sanity: mutual exclusion guaranteed by clap"
            );
            assert!(
                self.download_dir.is_none(),
                "sanity: mutual exclusion guaranteed by clap"
            );
            info!("using already downloaded rts tarball");
            rts_path
        } else {
            bail!("you must provide either rts-path or s3-url");
        };

        Ok(rts_path)
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(not(test), expect(dead_code))]
pub struct MountSpec {
    pub orb_mount_name: String,
    pub host_path: Utf8PathBuf,
}

#[derive(Debug, Error)]
pub enum MountSpecParseError {
    #[error("--mount expects <orb_mount_name>,<host_path>")]
    InvalidFormat,
}

impl FromStr for MountSpec {
    type Err = MountSpecParseError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let s = s.trim();
        if s.chars().any(char::is_whitespace) {
            return Err(MountSpecParseError::InvalidFormat);
        }

        let (left, right) = s
            .split_once(',')
            .ok_or(MountSpecParseError::InvalidFormat)?;

        if left.is_empty() || right.is_empty() {
            return Err(MountSpecParseError::InvalidFormat);
        }

        Ok(MountSpec {
            orb_mount_name: left.to_string(),
            host_path: Utf8PathBuf::from(right),
        })
    }
}

fn is_nixos_etc_release(s: &str) -> Result<bool> {
    let osr = etc_os_release::OsRelease::from_str(s)
        .wrap_err("failed to parse as an /etc/os-release file")?;
    let id = osr.id().to_lowercase();

    Ok(id == "nixos")
}

async fn error_detection_for_host_state() -> Result<()> {
    const USE_NIXOS: &str =
        "make sure this computer is running on a recent orb-software NixOS flake";
    if !crate::boot::is_recovery_mode_detected().await? {
        return Err(eyre!("orb must be in recovery mode to flash."))
            .with_suggestion(|| "Try running `orb-hil reboot -r`");
    }

    let is_nixos = tokio::fs::read_to_string("/etc/os-release")
        .await
        .wrap_err("failed to read /etc/os-release")
        .and_then(|s| is_nixos_etc_release(&s))
        .with_suggestion(|| "you are on a linux machine, right?")?;
    if !is_nixos {
        warn!(
            "warning - orb-hil nfsboot is only officially supported on a NixOS machine.
            We recommend installing the orb-software NixOS flake on your machine, as it
            already has all the configuration necessary."
        );
    }

    tokio::fs::read_to_string("/etc/exports")
        .await
        .wrap_err("failed to read /etc/exports")
        .with_note(|| "you should be running an NFS server")
        .and_then(|s| {
            check_exports(&s)
                .then_some(())
                .ok_or_eyre("/etc/exports was misconfigured")
        })
        .with_suggestion(|| USE_NIXOS)?;

    spawn_blocking(|| run_fun!(systemctl is-active nfs-server.service))
        .await
        .expect("task panicked")
        .wrap_err("error while checking for nfs-server systemd service")
        .with_suggestion(|| USE_NIXOS)?;

    spawn_blocking(|| run_fun!(sudo mount))
        .await
        .expect("task panicked")
        .wrap_err("error while checking for ability to mount")
        .with_suggestion(|| "make sure you have sudo rights")
        .with_note(|| "note that `nix run github:worldcoin/orb-software#tegra-bash` should *not* be used for the nfsboot command")
        .with_suggestion(|| "try using `nix develop github:worldcoin/orb-software#nfsboot`")?;

    Ok(())
}

// best-effort only.
fn check_exports(etc_exports_content: &str) -> bool {
    etc_exports_content
        .trim()
        .lines()
        .any(|line| line.trim().starts_with("/srv"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_exports() {
        const GOOD_EXAMPLE: &str = r##"/srv 10.42.0.0/24(rw,fsid=0,no_subtree_check,no_root_squash,crossmnt) # orbeth0 subnet"##;
        const BAD_EXAMPLE: &str = r##"/foobar 10.42.0.0/24(rw,fsid=0,no_subtree_check,no_root_squash,crossmnt) # orbeth0 subnet"##;
        assert!(check_exports(GOOD_EXAMPLE));
        assert!(!check_exports(""));
        assert!(!check_exports(BAD_EXAMPLE));
    }

    #[test]
    fn mountspec_parses_valid() {
        let m: MountSpec = "data,/var/tmp".parse().expect("should parse");
        assert_eq!(m.orb_mount_name, "data");
        assert_eq!(m.host_path, Utf8PathBuf::from("/var/tmp"));
    }

    #[test]
    fn mountspec_rejects_missing_comma() {
        let m: Result<MountSpec, MountSpecParseError> = "foo".parse();
        assert!(matches!(m, Err(MountSpecParseError::InvalidFormat)));
    }

    #[test]
    fn mountspec_rejects_empty_left() {
        let m: Result<MountSpec, MountSpecParseError> = ",/path".parse();
        assert!(matches!(m, Err(MountSpecParseError::InvalidFormat)));
    }

    #[test]
    fn mountspec_rejects_empty_right() {
        let m: Result<MountSpec, MountSpecParseError> = "name,".parse();
        assert!(matches!(m, Err(MountSpecParseError::InvalidFormat)));
    }

    #[test]
    fn mountspec_rejects_space_after_comma() {
        let m: Result<MountSpec, MountSpecParseError> = "foo, bar".parse();
        assert!(matches!(m, Err(MountSpecParseError::InvalidFormat)));
    }

    #[test]
    fn mountspec_rejects_space_before_comma() {
        let m: Result<MountSpec, MountSpecParseError> = "foo ,bar".parse();
        assert!(matches!(m, Err(MountSpecParseError::InvalidFormat)));
    }

    #[test]
    fn test_etc_osrelease() {
        const EXAMPLE: &str = r##"
ANSI_COLOR="0;38;2;126;186;228"
BUG_REPORT_URL="https://github.com/NixOS/nixpkgs/issues"
BUILD_ID="25.05.20250712.650e572"
CPE_NAME="cpe:/o:nixos:nixos:25.05"
DEFAULT_HOSTNAME=nixos
DOCUMENTATION_URL="https://nixos.org/learn.html"
HOME_URL="https://nixos.org/"
ID=nixos
ID_LIKE=""
IMAGE_ID=""
IMAGE_VERSION=""
LOGO="nix-snowflake"
NAME=NixOS
PRETTY_NAME="NixOS 25.05 (Warbler)"
SUPPORT_END="2025-12-31"
SUPPORT_URL="https://nixos.org/community.html"
VARIANT=""
VARIANT_ID=""
VENDOR_NAME=NixOS
VENDOR_URL="https://nixos.org/"
VERSION="25.05 (Warbler)"
VERSION_CODENAME=warbler
VERSION_ID="25.05"
"##;

        const EXAMPLE_ORB: &str = r##"
PRETTY_NAME="Ubuntu 22.04.5 LTS"
NAME="Ubuntu"
VERSION_ID="22.04"
VERSION="22.04.5 LTS (Jammy Jellyfish)"
VERSION_CODENAME=jammy
ID=ubuntu
ID_LIKE=debian
HOME_URL="https://www.ubuntu.com/"
SUPPORT_URL="https://help.ubuntu.com/"
BUG_REPORT_URL="https://bugs.launchpad.net/ubuntu/"
PRIVACY_POLICY_URL="https://www.ubuntu.com/legal/terms-and-policies/privacy-policy"
UBUNTU_CODENAME=jammy
ORB_OS_RELEASE_TYPE=dev
ORB_OS_PLATFORM_TYPE=diamond
ORB_OS_EXPECTED_MAIN_MCU_VERSION=v3.1.15
ORB_OS_EXPECTED_SEC_MCU_VERSION=v3.1.15
        "##;
        assert!(is_nixos_etc_release(EXAMPLE).unwrap());
        assert!(!is_nixos_etc_release(EXAMPLE_ORB).unwrap());
    }
}
