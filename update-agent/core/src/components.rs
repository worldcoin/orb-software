use std::os::unix::fs::MetadataExt;
use std::{collections::HashMap, fmt, fmt::Display, fs::File, io, path::PathBuf};

use serde::de;
use serde::de::Visitor;
use serde::Deserializer;

use gpt::{disk::LogicalBlockSize, partition::Partition, DiskDevice, GptDisk};
use rustix::fs::{major, minor};
use serde::{Deserialize, Serialize};

use super::Slot;

pub type Components = HashMap<String, Component>;

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Redundancy {
    #[serde(rename = "single")]
    Single,
    #[serde(rename = "redundant")]
    Redundant,
}

pub enum Location {
    Jetson,
    Mcu,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Device {
    #[serde(rename = "ssd")]
    Ssd,
    #[serde(rename = "qspi")]
    Qspi,
}

// Custom deserializer for Device that handles backward compatibility.
// The manifest may contain "emmc" or "nvme" as device types, but they all
// represent the same underlying SSD storage type. This deserializer transforms all
// three values into Device::Ssd to unify the representation.
impl<'de> Deserialize<'de> for Device {
    fn deserialize<D>(deserializer: D) -> Result<Device, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct DeviceVisitor;

        impl<'de> Visitor<'de> for DeviceVisitor {
            type Value = Device;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a valid device type (emmc, nvme, qspi, ssd)")
            }

            fn visit_str<E>(self, value: &str) -> Result<Device, E>
            where
                E: de::Error,
            {
                match value {
                    "emmc" | "nvme" | "ssd" => Ok(Device::Ssd),
                    "qspi" => Ok(Device::Qspi),
                    _ => Err(de::Error::unknown_variant(
                        value,
                        &["emmc", "nvme", "qspi", "ssd"],
                    )),
                }
            }
        }

        deserializer.deserialize_str(DeviceVisitor)
    }
}

/// Finds the block device where the given mountpoint is mounted.
///
/// This function uses stat to get the device major and minor numbers of the mountpoint,
/// then determines the parent block device.
///
/// That function is an eqivalent of this bash script
///
/// dev=$(stat -c '%d' <mountpoint>)
/// major=$((dev / 256))
/// minor=$((dev % 256))
/// sysfs_path="/sys/dev/block/$major:$minor"
/// link_target=$(readlink -f $sysfs_path)
/// device_name=$(basename $(dirname $link_target))
/// echo "/dev/$device_name"
fn find_block_device_by_mountpoint(
    mountpoint: &std::path::Path,
) -> std::io::Result<PathBuf> {
    // 'stat' the mountpoint. (see man 2 stat)
    let metadata = std::fs::metadata(mountpoint)?;

    // Get major & minor of the underlying device
    let dev = metadata.dev();
    let major = major(dev);
    let minor = minor(dev);

    // Construct the path in sysfs to find device information
    // (see man 5 sysfs, section on '/sys/dev/')
    let sysfs_path = format!("/sys/dev/block/{major}:{minor}");

    // Read the symlink to get the actual device path
    let link_target = std::fs::read_link(&sysfs_path)?;
    // The link target looks like: ../../devices/.../block/nvme0n1/nvme0n1p1
    // We want to get the parent directory name (nvme0n1 in this case)
    let device_name = link_target.parent().and_then(|x| x.file_name()).unwrap();
    // Construct the full device path
    let mut ret = PathBuf::from("/dev/");
    ret.push(device_name);
    Ok(ret)
}

pub fn find_root_blockdevice() -> std::io::Result<PathBuf> {
    find_block_device_by_mountpoint(std::path::Path::new("/usr/persistent"))
}

impl Device {
    fn to_path(&self) -> PathBuf {
        match self {
            Device::Ssd => {
                find_root_blockdevice().expect("Failed to guess root block device")
            }
            Device::Qspi => PathBuf::from("/dev/mtdblock0"),
        }
    }
}

impl Display for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_path().display())
    }
}

/// See documentation on structs for more info about the different variants.
#[derive(Deserialize, Serialize)]
#[serde(tag = "type", content = "value")]
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum Component {
    #[serde(rename = "can")]
    Can(Can),
    #[serde(rename = "gpt")]
    Gpt(Gpt),
    #[serde(rename = "raw")]
    Raw(Raw),
    #[serde(rename = "capsule")]
    Capsule(Capsule),
}

impl Component {
    pub fn redundancy(&self) -> Redundancy {
        match self {
            Self::Can(c) => c.redundancy(),
            Self::Gpt(c) => c.redundancy(),
            Self::Raw(c) => c.redundancy(),
            Self::Capsule(_) => Redundancy::Single,
        }
    }

    pub fn is_redundant(&self) -> bool {
        match self {
            Self::Can(c) => c.is_redundant(),
            Self::Gpt(c) => c.is_redundant(),
            Self::Raw(c) => c.is_redundant(),
            Self::Capsule(_) => false,
        }
    }
}

macro_rules! impl_is_redundant {
    ($($t:ty),+ $(,)?) => {
        $(
            impl $t {
                pub fn redundancy(&self) -> Redundancy {
                    self.redundancy
                }

                pub fn is_redundant(&self) -> bool {
                    match self.redundancy {
                        Redundancy::Single => false,
                        Redundancy::Redundant => true,
                    }
                }
            }
        )+
    }
}
impl_is_redundant!(Can, Gpt, Raw);

/// Firmware update to be sent over can.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Can {
    pub address: u32,
    pub bus: String,
    pub redundancy: Redundancy,
}

/// Block-level partition, that should be `dd`ed to the offsets described by the
/// orb's GPT partition tabel. This component comes with a GPT label to identify itself.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Gpt {
    pub device: Device,
    pub label: String,
    pub redundancy: Redundancy,
}

/// Raw block-level data that should be `dd`ed to a particular location on disk.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Raw {
    pub device: Device,
    pub offset: u64,
    pub size: u64,
    pub redundancy: Redundancy,
}

/// A capsule update.
/// See <https://docs.nvidia.com/jetson/archives/r35.4.1/DeveloperGuide/text/SD/Bootloader/UpdateAndRedundancy.html#generating-the-capsule-update-payload>
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Capsule {}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed opening GPT disk at `{path}`")]
    OpenGptDisk { path: String, source: gpt::GptError },
    #[error("failed opening raw file at `{path}`")]
    OpenRawFile { path: String, source: io::Error },
    #[error("failed matching partition with label `{label}`")]
    GetGptPartition { label: String },
}

impl Gpt {
    pub const LOGICAL_BLOCK_SIZE: LogicalBlockSize = LogicalBlockSize::Lb512;

    /// Possible partition labels matching the component and slot.
    ///
    /// Pearl: "<component.NAME>\_<slot>" || Diamond: "<SLOT>\_<component.NAME>"
    fn get_partition_name(&self, slot: Slot) -> Vec<String> {
        match self.redundancy {
            Redundancy::Redundant => {
                vec![
                    format!(
                        "{}_{}",
                        slot.to_string().to_uppercase(),
                        self.label.clone()
                    ),
                    format!("{}_{}", self.label.clone(), slot.to_string()),
                ]
            }

            Redundancy::Single => vec![self.label.clone()],
        }
    }

    /// The disk that contains this partition
    pub fn get_disk(&self, writeable: bool) -> Result<GptDisk<File>, Error> {
        gpt::GptConfig::new()
            .writable(writeable)
            .logical_block_size(Self::LOGICAL_BLOCK_SIZE)
            .open(self.device.to_path())
            .map_err(|source| Error::OpenGptDisk {
                path: self.device.to_string(),
                source,
            })
    }

    /// Reads this componen't partition entry from `disk`
    pub fn read_partition_entry(
        &self,
        disk: &GptDisk<impl DiskDevice>,
        slot: Slot,
    ) -> Result<Partition, Error> {
        let part_names = self.get_partition_name(slot);

        let part = disk
            .partitions()
            .iter()
            .find_map(|(_, p)| {
                part_names
                    .iter()
                    .any(|part_name| part_name.eq(&p.name))
                    .then(|| p.clone())
            })
            .ok_or(Error::GetGptPartition {
                label: part_names.join(" or "),
            })?;

        Ok(part)
    }
}

impl Raw {
    pub fn get_file(&self) -> Result<File, Error> {
        File::options()
            .read(true)
            .write(true)
            .create(false)
            .open(self.device.to_path())
            .map_err(|source| Error::OpenRawFile {
                path: self.device.to_string(),
                source,
            })
    }
}
