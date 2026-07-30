// The `squashfs` partitions are setup with dm-verity to guarantee
// attestation of the running software and prevent errors of corruption.
// Their integrity is lazily evaluated against the verity hash trees
// computed by the build process of `orb-os`.
// We ensure partition integrity before accepting the boot slot. In case
// of a block integrity mismatch, the kernel will deliberately panic and
// the device will reboot until falling back to the previous slot &
// attempt to OTA again.

use std::{
    io::{self, Read},
    path::{Path, PathBuf},
    process::Command,
};

use flate2::read::GzDecoder;
use orb_info::orb_os_release::OrbOsPlatform;
use sys_mount::{Mount, MountFlags, UnmountFlags};
use thiserror::Error;

const VERITY_VARIABLES_PATH: &str = "verity_variables.env";

#[derive(Debug, Error)]
pub(crate) enum VerityError {
    #[error("invalid verity metadata")]
    InvalidConfig,
    #[error("block device does not exist: {}", .0.display())]
    MissingDevice(PathBuf),
    #[error("failed to read verity metadata source")]
    ReadSource(#[source] io::Error),
    #[error("failed to run `veritysetup verify`: {0}")]
    Veritysetup(#[source] io::Error),
}

/// `validate_verity` executes an eager integrity check on the rootfs of the device against the
/// dm-verity hash tree embedded to the relevant partitions during the build process of `orb-os`.
pub(crate) fn validate_verity(platform: OrbOsPlatform) -> Result<(), VerityError> {
    if platform == OrbOsPlatform::Pearl {
        return Ok(());
    }

    let devices = match platform {
        OrbOsPlatform::Diamond => {
            let cmdline = std::fs::File::open("/proc/cmdline")
                .map_err(VerityError::ReadSource)?;
            vec![DeviceConfig::from_cmdline(
                cmdline,
                Path::new("/dev/disk/by-partlabel/SYSTEM"),
            )?]
        }
        OrbOsPlatform::Pearl => DeviceConfig::from_initrd(
            // These are pretty much __set in stone__
            // TODO: consider fetching the maps dynamically
            // because this looks somewhat ugly
            Path::new("/dev/disk/by-partlabel/APP"),
            &[
                "/dev/disk/by-partlabel/AI_LAYER",
                "/dev/disk/by-partlabel/BASE_LAYER",
                "/dev/disk/by-partlabel/CACHE_LAYER",
                "/dev/disk/by-partlabel/CUDA_LAYER",
                "/dev/disk/by-partlabel/LFT_LAYER",
                "/dev/disk/by-partlabel/PACKAGES_LAYER",
                "/dev/disk/by-partlabel/SECURITY_LAYER",
                "/dev/disk/by-partlabel/SOFTWARE_LAYER",
                "/dev/disk/by-partlabel/SYSTEM_LAYER",
            ],
        )?,
    };

    devices.iter().try_for_each(DeviceConfig::validate)
}

struct DeviceConfig<'a> {
    device: &'a Path,
    root_hash: [u8; 32],
    data_blocks: u64,
    hash_offset: u64,
}

impl<'a> DeviceConfig<'a> {
    fn validate(&self) -> Result<(), VerityError> {
        if !self.device.exists() {
            return Err(VerityError::MissingDevice(self.device.into()));
        }

        let output = Command::new("veritysetup")
            .arg("verify")
            .arg(format!("--hash-offset={}", self.hash_offset))
            .arg(format!("--data-blocks={}", self.data_blocks))
            .arg(self.device)
            .arg(self.device)
            .arg(hex::encode(self.root_hash))
            .output()
            .map_err(VerityError::Veritysetup)?;

        if !output.status.success() {
            return Err(VerityError::Veritysetup(io::Error::other(format!(
                "exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ))));
        }

        Ok(())
    }

    fn from_cmdline(source: impl Read, device: &'a Path) -> Result<Self, VerityError> {
        let cmdline = io::read_to_string(source).map_err(VerityError::ReadSource)?;

        let root_hash = cmdline_value(&cmdline, "VERITY_ROOT_HASH")?;
        let mut root_hash_bytes = [0; 32];
        hex::decode_to_slice(root_hash, &mut root_hash_bytes)
            .map_err(|_| VerityError::InvalidConfig)?;

        let data_blocks = cmdline_value(&cmdline, "VERITY_DATA_BLOCKS")?
            .parse()
            .map_err(|_| VerityError::InvalidConfig)?;

        let hash_offset = cmdline_value(&cmdline, "VERITY_HASH_OFFSET")?
            .parse()
            .map_err(|_| VerityError::InvalidConfig)?;

        Ok(Self {
            device,
            root_hash: root_hash_bytes,
            data_blocks,
            hash_offset,
        })
    }

    fn from_initrd(
        app_device: &Path,
        devices: &'a [&str],
    ) -> Result<Vec<Self>, VerityError> {
        let mount_point = tempfile::tempdir().map_err(VerityError::ReadSource)?;
        let mut initrd = Vec::new();
        {
            let _mount = Mount::builder()
                .fstype("vfat")
                .flags(
                    MountFlags::RDONLY
                        | MountFlags::NOEXEC
                        | MountFlags::NODEV
                        | MountFlags::NOSUID,
                )
                .mount_autodrop(app_device, mount_point.path(), UnmountFlags::empty())
                .map_err(VerityError::ReadSource)?;

            let initrd_fp =
                std::fs::File::open(mount_point.path().join("app/boot/initrd"))
                    .map_err(VerityError::ReadSource)?;
            GzDecoder::new(initrd_fp)
                .read_to_end(&mut initrd)
                .map_err(VerityError::ReadSource)?;
        }

        let verity_variables = cpio_reader::iter_files(&initrd)
            .find(|entry| entry.name() == VERITY_VARIABLES_PATH)
            .ok_or(VerityError::InvalidConfig)
            .and_then(|entry| {
                std::str::from_utf8(entry.file())
                    .map_err(|_| VerityError::InvalidConfig)
            })?;

        parse_verity_variables(verity_variables, devices)
    }
}

// Helper function; searches for `" name=value "` in an str & returns `value`
fn cmdline_value<'a>(cmdline: &'a str, name: &str) -> Result<&'a str, VerityError> {
    for argument in cmdline.split_whitespace() {
        let Some((key, value)) = argument.split_once('=') else {
            continue;
        };
        if key == name && !value.is_empty() {
            return Ok(value);
        }
    }

    Err(VerityError::InvalidConfig)
}

fn parse_verity_variables<'a>(
    variables: &str,
    devices: &'a [&str],
) -> Result<Vec<DeviceConfig<'a>>, VerityError> {
    devices
        .iter()
        .map(|device| {
            let layer = device
                .strip_prefix("/dev/disk/by-partlabel/")
                .and_then(|name| name.strip_suffix("_LAYER"))
                .ok_or(VerityError::InvalidConfig)?;

            let root_hash = initrd_value(variables, &format!("{layer}_VERITY_HASH"))?;
            let mut root_hash_bytes = [0; 32];
            hex::decode_to_slice(root_hash, &mut root_hash_bytes)
                .map_err(|_| VerityError::InvalidConfig)?;

            let data_blocks = initrd_value(variables, &format!("{layer}_DATA_BLOCKS"))?
                .parse()
                .map_err(|_| VerityError::InvalidConfig)?;

            let hash_offset = initrd_value(variables, &format!("{layer}_HASH_OFFSET"))?
                .parse()
                .map_err(|_| VerityError::InvalidConfig)?;

            Ok(DeviceConfig {
                device: Path::new(device),
                root_hash: root_hash_bytes,
                data_blocks,
                hash_offset,
            })
        })
        .collect()
}

fn initrd_value<'a>(variables: &'a str, name: &str) -> Result<&'a str, VerityError> {
    variables
        .lines()
        .rev()
        .find_map(|line| {
            let (key, value) = line.strip_prefix("export ")?.split_once('=')?;
            let value = value.strip_prefix('\'')?.strip_suffix('\'')?;
            (key == name && !value.is_empty()).then_some(value)
        })
        .ok_or(VerityError::InvalidConfig)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIAMOND_CMDLINE: &str =
        "root=/dev/mapper/root net.ifnames=0 systemd.setenv=CURRENT_BOOT_SLOT=b \
         systemd.hostname=orb-6E3BE65C systemd.setenv=ORB_ID=6E3BE65C \
         VERITY_ROOT_HASH=1bb6ed665009f74c428725aa817b7b93244facfa778b41edb453c802025cf636 \
         VERITY_DATA_BLOCKS=1351936 VERITY_HASH_OFFSET=5537529856";

    #[test]
    fn parses_diamond_cmdline() -> Result<(), VerityError> {
        let config = DeviceConfig::from_cmdline(
            DIAMOND_CMDLINE.as_bytes(),
            Path::new("/dev/disk/by-partlabel/SYSTEM"),
        )?;

        assert_eq!(
            hex::encode(config.root_hash),
            "1bb6ed665009f74c428725aa817b7b93244facfa778b41edb453c802025cf636",
        );
        assert_eq!(config.data_blocks, 1_351_936);
        assert_eq!(config.hash_offset, 5_537_529_856);

        Ok(())
    }

    #[test]
    fn rejects_invalid_data_block_count() -> Result<(), VerityError> {
        let cmdline = DIAMOND_CMDLINE.replace("1351936", "x");

        assert!(matches!(
            DeviceConfig::from_cmdline(cmdline.as_bytes(), Path::new("")),
            Err(VerityError::InvalidConfig)
        ));

        Ok(())
    }

    #[test]
    fn parses_verity_variables() -> Result<(), VerityError> {
        let config = parse_verity_variables(
            "export SYSTEM_VERITY_HASH='55ecce07f56e4ac7156b56f5583b6ade8341eff9ec5b2178e1ebcfc036a48c84'\n\
             export SYSTEM_DATA_BLOCKS='13137'\n\
             export SYSTEM_HASH_OFFSET='53809152'",
            &["/dev/disk/by-partlabel/SYSTEM_LAYER"],
        )?
        .pop()
        .ok_or(VerityError::InvalidConfig)?;

        assert_eq!(
            hex::encode(config.root_hash),
            "55ecce07f56e4ac7156b56f5583b6ade8341eff9ec5b2178e1ebcfc036a48c84",
        );
        assert_eq!(config.data_blocks, 13_137);
        assert_eq!(config.hash_offset, 53_809_152);

        Ok(())
    }
}
