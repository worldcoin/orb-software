use std::{collections::HashMap, fmt::Display, fs::File, io, path::PathBuf};

use gpt::{disk::LogicalBlockSize, partition::Partition, GptDisk};
use serde::{Deserialize, Serialize};

use super::Slot;

pub type Components = HashMap<String, Component>;

const LOGICAL_BLOCK_SIZE: LogicalBlockSize = LogicalBlockSize::Lb512;

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Deserialize, Serialize, Clone, Debug)]
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

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", content = "value")]
#[derive(Clone, Debug)]
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

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Can {
    pub address: u32,
    pub bus: String,
    pub redundancy: Redundancy,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Gpt {
    pub device: Device,
    pub label: String,
    pub redundancy: Redundancy,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Raw {
    pub device: Device,
    pub offset: u64,
    pub size: u64,
    pub redundancy: Redundancy,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Capsule {}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed opening GPT disk at `{path}`")]
    OpenGptDisk { path: String, source: io::Error },
    #[error("failed opening raw file at `{path}`")]
    OpenRawFile { path: String, source: io::Error },
    #[error("failed matching partition with label `{label}`")]
    GetGptPartition { label: String },
}

impl Gpt {
    fn get_partition_name(&self, slot: Slot) -> String {
        match self.redundancy {
            Redundancy::Redundant => format!("{}_{}", self.label.clone(), slot),
            Redundancy::Single => self.label.clone(),
        }
    }

    pub fn get_disk(&self) -> Result<GptDisk, Error> {
        gpt::GptConfig::new()
            .writable(true)
            .initialized(true)
            .logical_block_size(LOGICAL_BLOCK_SIZE)
            .open(self.device.to_path())
            .map_err(|source| Error::OpenGptDisk {
                path: self.device.to_string(),
                source,
            })
    }

    pub fn get_partition(
        &self,
        disk: &GptDisk,
        slot: Slot,
    ) -> Result<Partition, Error> {
        let part_name = self.get_partition_name(slot);

        let part = disk
            .partitions()
            .iter()
            .find_map(|(_, p)| part_name.eq(&p.name).then(|| p.clone()))
            .ok_or(Error::GetGptPartition { label: part_name })?;

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

mod private {
    pub trait Sealed {}
}
pub trait PartitionExt: private::Sealed {
    fn contents_range(&self) -> io::Result<std::ops::Range<u64>>;
}

impl private::Sealed for ::gpt::partition::Partition {}
impl PartitionExt for ::gpt::partition::Partition {
    fn contents_range(&self) -> io::Result<std::ops::Range<u64>> {
        let start = self.bytes_start(LOGICAL_BLOCK_SIZE)?;
        let size = self.bytes_len(LOGICAL_BLOCK_SIZE)?;
        Ok(start..(start + size))
    }
}
