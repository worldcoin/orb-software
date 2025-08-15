use std::path::{Path, PathBuf};
use std::str::FromStr;

use camino::Utf8PathBuf;
use cmd_lib::run_fun;
use color_eyre::eyre::{ensure, eyre, Context as _, OptionExt as _};
use color_eyre::{Result, Section as _};
use thiserror::Error;
use tokio::task::spawn_blocking;
use tracing::{debug, warn};

use crate::rts::extract;

pub const USE_NIXOS: &str =
    "make sure this computer is running on a recent orb-software NixOS flake";
pub const NO_TEGRA_BASH: &str = "`nix run github:worldcoin/orb-software#tegra-bash` should *not* be used for the nfsboot command";
pub const USE_NFS_DEVSHELL: &str =
    "try using `nix develop github:worldcoin/orb-software#nfsboot`";

// TODO: How to handle /usr/persistent? Is it even necessary?
pub async fn nfsboot(path_to_rts: Utf8PathBuf, mounts: Vec<MountSpec>) -> Result<()> {
    let tmp_dir = tokio::task::spawn_blocking(move || extract(&path_to_rts))
        .await
        .wrap_err("task panicked")??;
    debug!("{tmp_dir:?}");

    let scratch_dir = tmp_dir.path().join("scratch");
    tokio::fs::create_dir(&scratch_dir)
        .await
        .wrap_err_with(|| format!("failed to create {scratch_dir:?}"))?;

    tokio::task::spawn_blocking(move || {
        do_mounting(&tmp_dir.path().join("rts"), &scratch_dir, &mounts)
    })
    .await
    .wrap_err("task panicked")?
    .wrap_err("failed to to the mounting ritual")?;

    todo!("finish the rest after the mounting")
}

fn do_mounting(rts_dir: &Path, scratch_dir: &Path, mounts: &[MountSpec]) -> Result<()> {
    assert!(rts_dir.exists(), "rts_dir is expected to exist");
    assert!(scratch_dir.exists(), "scratch_dir is expected to exist");

    let rootfs_path = rts_dir.join("rootfs.sqfs");
    let sqfs_mnt = scratch_dir.join("sqfs");
    let upperdir = scratch_dir.join("upperdir");
    let workdir = scratch_dir.join("workdir");
    let overlay_mnt = scratch_dir.join("overlay");
    for d in [&sqfs_mnt, &upperdir, &workdir, &overlay_mnt] {
        std::fs::create_dir(d).wrap_err_with(|| format!("failed to create {d:?}"))?;
    }

    regular_mount(&rootfs_path, &sqfs_mnt)
        .wrap_err("failed to mount rootfs squashfs")?;
    overlay_mount()
        .lower(&sqfs_mnt)
        .upper(&upperdir)
        .workdir(&workdir)
        .to(&overlay_mnt)
        .call()
        .wrap_err("failed to create overlay mount on top of rootfs")?;

    let inner_mount_dir = overlay_mnt.join("mnt");
    ensure!(
        inner_mount_dir.exists(),
        "/mnt in the rootfs should always exist"
    );
    for d in mounts
        .iter()
        .map(|m| inner_mount_dir.join(&m.orb_mount_name))
    {
        run_fun!(sudo mkdir $d).wrap_err_with(|| format!("failed to create {d:?}"))?;
    }

    let srv_dir = PathBuf::from("/srv");
    bind_mount(&overlay_mnt, &srv_dir)
        .wrap_err_with(|| format!("failed to bind mount overlay onto {srv_dir:?}"))?;

    let srv_mnt_dir = srv_dir.join("mnt");
    for m in mounts {
        let p = &m.host_path;
        bind_mount(p.as_ref(), &srv_mnt_dir.join(&m.orb_mount_name))
            .wrap_err_with(|| format!("failed to bind mount user-specified dir {p}"))?;
    }

    Ok(())
}

fn bind_mount(from: &Path, to: &Path) -> Result<()> {
    run_fun!(sudo mount --bind $from $to)
        .wrap_err_with(|| format!("failed to bind mount {from:?} to {to:?}"))?;

    Ok(())
}

#[bon::builder]
fn overlay_mount(lower: &Path, upper: &Path, workdir: &Path, to: &Path) -> Result<()> {
    let options = format!(
        "lowerdir={},upperdir={},workdir={},index=on,nfs_export=on",
        lower.display(),
        upper.display(),
        workdir.display()
    );
    run_fun!(
        sudo mount -t overlay overlay -o $options $to
    )
    .wrap_err_with(|| {
        format!(
            "failed to mount an overlay with \
            lower={lower:?}, \
            upper={upper:?}, \
            workdir={workdir:?} to {to:?}"
        )
    })?;

    Ok(())
}

fn regular_mount(from: &Path, to: &Path) -> Result<()> {
    run_fun!(sudo mount $from $to)
        .wrap_err_with(|| format!("failed to mount {from:?} to {to:?}"))?;

    Ok(())
}

#[derive(Debug, Clone)]
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

pub async fn error_detection_for_host_state() -> Result<()> {
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

    spawn_blocking(|| run_fun!(sudo mount))
        .await
        .expect("task panicked")
        .wrap_err("error while checking for ability to mount")
        .with_suggestion(|| "make sure you have sudo rights")
        .with_note(|| NO_TEGRA_BASH)
        .with_suggestion(|| USE_NFS_DEVSHELL)?;

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

    if !crate::boot::is_recovery_mode_detected().await? {
        return Err(eyre!("orb must be in recovery mode to flash."))
            .with_suggestion(|| "Try running `orb-hil reboot -r`");
    }

    Ok(())
}

fn is_nixos_etc_release(s: &str) -> Result<bool> {
    let osr = etc_os_release::OsRelease::from_str(s)
        .wrap_err("failed to parse as an /etc/os-release file")?;
    let id = osr.id().to_lowercase();

    Ok(id == "nixos")
}

// best-effort only.
fn check_exports(etc_exports_content: &str) -> bool {
    etc_exports_content
        .trim()
        .lines()
        .any(|line| line.trim().starts_with("/srv"))
}

pub async fn request_sudo() -> Result<()> {
    spawn_blocking(|| run_fun!(sudo -l;))
        .await
        .expect("task panicked")
        .wrap_err("failed to request sudo rights")
        .with_note(|| NO_TEGRA_BASH)
        .with_suggestion(|| USE_NFS_DEVSHELL)?;

    Ok(())
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
