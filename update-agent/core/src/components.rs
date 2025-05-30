use std::{collections::HashMap, fmt::Display, fs::File, io, path::PathBuf};

use gpt::{disk::LogicalBlockSize, partition::Partition, DiskDevice, GptDisk};
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

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Device {
    #[serde(rename = "emmc")]
    Emmc,
    #[serde(rename = "nvme")]
    Nvme,
    #[serde(rename = "qspi")]
    Qspi,
}

impl Device {
    fn to_path(&self) -> PathBuf {
        match self {
            Self::Emmc => PathBuf::from(&self.to_string()),
            Self::Nvme => PathBuf::from(&self.to_string()),
            Self::Qspi => PathBuf::from(&self.to_string()),
        }
    }
}

impl Display for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Device::Emmc => "/dev/mmcblk0",
                Device::Nvme => "/dev/nvme0n1",
                Device::Qspi => "/dev/mtdblock0",
            }
        )
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
