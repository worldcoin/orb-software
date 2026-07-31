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

use fatfs::{FileSystem, FsOptions};
use flate2::read::GzDecoder;
use orb_dogd::{MetricEmitter, NO_TAGS};
use orb_info::orb_os_release::OrbOsPlatform;
use std::time::Instant;
use thiserror::Error;

const VERITY_VARIABLES_PATH: &str = "verity_variables.env";
const METRIC_VALIDATION_DURATION: &str =
    "orb.platform.update-verifier.verity_validation_duration";
const METRIC_VALIDATION_FAILURE: &str =
    "orb.platform.update-verifier.verity_validation_failure";

#[derive(Debug, Error)]
pub enum VerityError {
    #[error("invalid verity metadata")]
    InvalidConfig,
    #[error("block device does not exist: {}", .0.display())]
    MissingDevice(PathBuf),
    #[error("failed to read verity metadata source")]
    ReadSource(#[source] io::Error),
    #[error("failed to run `veritysetup verify`: {0}")]
    Veritysetup(#[source] io::Error),
    #[error("verity validation on `{0}` failed: {1}")]
    VerificationFailed(PathBuf, String),
}

/// `validate_verity` executes an eager integrity check on the rootfs of the device against the
/// dm-verity hash tree embedded to the relevant partitions during the build process of `orb-os`.
pub fn validate_verity(
    platform: OrbOsPlatform,
    source_root: &Path,
    metrics: &impl MetricEmitter,
) -> Result<(), VerityError> {
    let start = Instant::now();
    match platform {
        OrbOsPlatform::Diamond => {
            let cmdline = std::fs::File::open(source_root.join("proc/cmdline"))
                .map_err(VerityError::ReadSource)?;
            let device = source_root.join("dev/disk/by-partlabel/SYSTEM");

            DeviceConfig::from_cmdline(cmdline, &device)?.validate()
        }
        OrbOsPlatform::Pearl => {
            // These are pretty much __"set in stone"__
            // no real reason the fetch them dynamically
            // other than for further testing infrastructure
            let app = source_root.join("dev/disk/by-partlabel/APP");
            let devices = [
                source_root.join("dev/disk/by-partlabel/AI_LAYER"),
                source_root.join("dev/disk/by-partlabel/BASE_LAYER"),
                source_root.join("dev/disk/by-partlabel/CACHE_LAYER"),
                source_root.join("dev/disk/by-partlabel/CUDA_LAYER"),
                source_root.join("dev/disk/by-partlabel/LFT_LAYER"),
                source_root.join("dev/disk/by-partlabel/PACKAGES_LAYER"),
                source_root.join("dev/disk/by-partlabel/SECURITY_LAYER"),
                source_root.join("dev/disk/by-partlabel/SOFTWARE_LAYER"),
                source_root.join("dev/disk/by-partlabel/SYSTEM_LAYER"),
            ];
            let device_paths = devices.each_ref().map(PathBuf::as_path);
            let devices = DeviceConfig::from_initrd(&app, &device_paths)?;

            devices.iter().try_for_each(DeviceConfig::validate)
        }
    }
    .inspect_err(|_| {
        let _ = metrics
            .incr(METRIC_VALIDATION_FAILURE, NO_TAGS)
            .inspect_err(|error| {
                tracing::error!(?error, "failed to emit verity failure metric")
            });
    })
    .inspect(|_| {
        let _ = metrics
            .dist(
                METRIC_VALIDATION_DURATION,
                start.elapsed().as_millis() as f64,
                NO_TAGS,
            )
            .inspect_err(|error| {
                tracing::error!(?error, "failed to emit verity duration metric")
            });
    })
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
            return Err(VerityError::VerificationFailed(
                self.device.into(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
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
        source: &Path,
        devices: &'a [&'a Path],
    ) -> Result<Vec<Self>, VerityError> {
        let filesystem = FileSystem::new(
            std::fs::File::open(source).map_err(VerityError::ReadSource)?,
            FsOptions::new().update_accessed_date(false),
        )
        .map_err(VerityError::ReadSource)?;

        let mut initrd = Vec::new();
        GzDecoder::new(
            filesystem
                .root_dir()
                .open_file("app/boot/initrd")
                .map_err(VerityError::ReadSource)?,
        )
        .read_to_end(&mut initrd)
        .map_err(VerityError::ReadSource)?;

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
    devices: &'a [&'a Path],
) -> Result<Vec<DeviceConfig<'a>>, VerityError> {
    devices
        .iter()
        .map(|device| {
            let layer = device
                .file_name()
                .and_then(|name| name.to_str())
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
                device,
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
        "root=/dev/mapper/root VERITY_ROOT_HASH=1bb6ed665009f74c428725aa817b7b93244facfa778b41edb453c802025cf636 \
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
        let devices = [Path::new("/dev/disk/by-partlabel/SYSTEM_LAYER")];
        let config = parse_verity_variables(
            "export SYSTEM_VERITY_HASH='55ecce07f56e4ac7156b56f5583b6ade8341eff9ec5b2178e1ebcfc036a48c84'\n\
             export SYSTEM_DATA_BLOCKS='13137'\n\
             export SYSTEM_HASH_OFFSET='53809152'",
            &devices,
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
