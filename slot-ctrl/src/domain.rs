use std::{fmt::Display, str::FromStr};

use derive_more::Display;
use efivar::EfiVarData;

pub type Result<T> = std::result::Result<T, Error>;

/// Error definition for library.
#[allow(missing_docs)]
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("failed reading efivar, invalid data length. expected: {expected}, actual: {actual}")]
    InvalidEfiVarLen { expected: usize, actual: usize },
    #[error("invalid slot configuration")]
    InvalidSlotData,
    #[error("invalid bootchain-firmware status")]
    InvalidBootChainFwStatusData,
    #[error("invalid rootfs status")]
    InvalidRootFsStatusData,
    #[error("failed opening scratch register: {0}")]
    CouldNotOpenScratchReg(String),
    #[error("invalid retry counter({counter}), exceeding the maximum ({max})")]
    ExceedingRetryCount { counter: u8, max: u8 },
    #[error("{0}")]
    EfiVar(#[from] color_eyre::Report),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[repr(u8)]
pub enum BootChainFwStatus {
    Success = 0,
    InProgress = 1,
    ErrorNoOpRequired = 2,
    ErrorFmpConflict = 3,
    ErrorReadingStatus = 4,
    ErrorMaxResetCount = 5,
    ErrorSettingResetCount = 6,
    ErrorSettingInProgress = 7,
    ErrorInProgressFailed = 8,
    ErrorBadBootChainNext = 9,
    ErrorReadingNext = 10,
    ErrorUpdatingFwChain = 11,
    ErrorBootChainFailed = 12,
    ErrorReadingResetCount = 13,
    ErrorBootNextExists = 14,
    ErrorReadingScratch = 15,
    ErrorSettingScratch = 16,
    ErrorUpdateBrBctFlagSet = 17,
    ErrorSettingPrevious = 18,
}

impl BootChainFwStatus {
    pub(crate) const STATUS_PATH: &str =
        "BootChainFwStatus-781e084c-a330-417c-b678-38e696380cb9";

    pub fn to_efivar_data(&self) -> EfiVarData {
        EfiVarData::new(0x7, [*self as u8, 0x0, 0x0, 0x0])
    }

    pub fn from_efivar_data(data: &EfiVarData) -> Result<Self> {
        let len = data.len();
        if len != 8 {
            return Err(Error::InvalidEfiVarLen {
                expected: 8,
                actual: len,
            });
        }

        Self::try_from(data.value()[0])
    }
}

impl TryFrom<u8> for BootChainFwStatus {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Success),
            1 => Ok(Self::InProgress),
            2 => Ok(Self::ErrorNoOpRequired),
            3 => Ok(Self::ErrorFmpConflict),
            4 => Ok(Self::ErrorReadingStatus),
            5 => Ok(Self::ErrorMaxResetCount),
            6 => Ok(Self::ErrorSettingResetCount),
            7 => Ok(Self::ErrorSettingInProgress),
            8 => Ok(Self::ErrorInProgressFailed),
            9 => Ok(Self::ErrorBadBootChainNext),
            10 => Ok(Self::ErrorReadingNext),
            11 => Ok(Self::ErrorUpdatingFwChain),
            12 => Ok(Self::ErrorBootChainFailed),
            13 => Ok(Self::ErrorReadingResetCount),
            14 => Ok(Self::ErrorBootNextExists),
            15 => Ok(Self::ErrorReadingScratch),
            16 => Ok(Self::ErrorSettingScratch),
            17 => Ok(Self::ErrorUpdateBrBctFlagSet),
            18 => Ok(Self::ErrorSettingPrevious),
            _ => Err(Error::InvalidBootChainFwStatusData),
        }
    }
}

/// Representation of the slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Display)]
#[repr(u8)]
pub enum Slot {
    #[display("a")]
    A = 0,
    #[display("b")]
    B = 1,
}

/// Representation of the rootfs status.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Display)]
#[repr(u8)]
pub enum RootFsStatus {
    /// Default status of the rootfs.
    Normal = 0x0,
    /// Rootfs status signifying that an update consumption has initiated
    UpdateInProcess = 0x1,
    /// Rootfs status signifying that an update was done & active slot switched
    UpdateDone = 0x2,
    /// Rootfs status signifying that the target slot is considered unbootable.
    Unbootable = 0x3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryCounts {
    pub efi_var: EfiRetryCount,
    pub scratch_reg: Option<ScratchRegRetryCount>,
}

impl Display for RetryCounts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const NA: &str = "unavailable in this platform";

        writeln!(f, "efi var: {}", &self.efi_var)?;

        // TODO: Remove once the pearl driver is patched
        write!(f, "scratch register: ")?;
        match self.scratch_reg {
            Some(v) => write!(f, "{v}")?,
            None => write!(f, "{NA}")?,
        };

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Display)]
pub struct ScratchRegRetryCount(pub u8);

impl ScratchRegRetryCount {
    pub(crate) const COUNT_A_PATH: &str =
        "sys/devices/platform/bus@0/c360000.pmc/rootfs_retry_count_a";
    pub(crate) const COUNT_B_PATH: &str =
        "sys/devices/platform/bus@0/c360000.pmc/rootfs_retry_count_b";
    pub(crate) const COUNT_MAX: u8 = 0x3;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Display)]
pub struct EfiRetryCount(pub u8);

impl EfiRetryCount {
    pub(crate) const COUNT_A_PATH: &str =
        "RootfsRetryCountA-781e084c-a330-417c-b678-38e696380cb9";
    pub(crate) const COUNT_B_PATH: &str =
        "RootfsRetryCountB-781e084c-a330-417c-b678-38e696380cb9";
    pub(crate) const COUNT_MAX_PATH: &str =
        "RootfsRetryCountMax-781e084c-a330-417c-b678-38e696380cb9";

    pub fn to_efivar_data(&self) -> EfiVarData {
        EfiVarData::new(0x7, [self.0, 0x0, 0x0, 0x0])
    }

    pub fn from_efivar_data(data: &EfiVarData) -> Result<EfiRetryCount> {
        let len = data.len();
        if len != 8 {
            return Err(Error::InvalidEfiVarLen {
                expected: 8,
                actual: len,
            });
        }

        // While the data part of the retry count EFI var is 4 bytes,
        // the retry count is only stored in the first byte.
        Ok(EfiRetryCount(data.value()[0]))
    }
}

impl FromStr for Slot {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "a" | "0" => Ok(Slot::A),
            "b" | "1" => Ok(Slot::B),
            _ => Err(Error::InvalidSlotData),
        }
    }
}

impl Slot {
    const SLOT_A_BYTES: [u8; 4] = [0x00, 0x00, 0x00, 0x00];
    const SLOT_B_BYTES: [u8; 4] = [0x01, 0x00, 0x00, 0x00];

    pub(crate) const CURRENT_SLOT_PATH: &str =
        "BootChainFwCurrent-781e084c-a330-417c-b678-38e696380cb9";
    pub(crate) const NEXT_SLOT_PATH: &str =
        "BootChainFwNext-781e084c-a330-417c-b678-38e696380cb9";

    /// Slot as EfiVar raw bytes
    pub fn to_efivar_data(&self) -> EfiVarData {
        EfiVarData::new(0x7, [*self as u8, 0x0, 0x0, 0x0])
    }

    pub fn from_efivar_data(data: &EfiVarData) -> Result<Slot> {
        if data.len() != 8 {
            return Err(Error::InvalidEfiVarLen {
                expected: 8,
                actual: data.len(),
            });
        }

        match data.value() {
            val if val == Self::SLOT_A_BYTES => Ok(Slot::A),
            val if val == Self::SLOT_B_BYTES => Ok(Slot::B),
            _ => Err(Error::InvalidSlotData),
        }
    }
}

impl FromStr for RootFsStatus {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "normal" | "0" => Ok(RootFsStatus::Normal),
            "updateinprocess" | "1" => Ok(RootFsStatus::UpdateInProcess),
            "updatedone" | "2" => Ok(RootFsStatus::UpdateDone),
            "unbootable" | "3" => Ok(RootFsStatus::Unbootable),
            _ => Err(Error::InvalidRootFsStatusData),
        }
    }
}

impl RootFsStatus {
    const NORMAL: [u8; 4] = [0x00, 0x00, 0x00, 0x00];
    const UPDATE_IN_PROGRESS: [u8; 4] = [0x01, 0x00, 0x00, 0x00];
    const UPDATE_DONE: [u8; 4] = [0x02, 0x00, 0x00, 0x00];
    const UNBOOTABLE: [u8; 4] = [0x03, 0x00, 0x00, 0x00];

    pub(crate) const STATUS_A_PATH: &str =
        "RootfsStatusSlotA-781e084c-a330-417c-b678-38e696380cb9";
    pub(crate) const STATUS_B_PATH: &str =
        "RootfsStatusSlotB-781e084c-a330-417c-b678-38e696380cb9";

    pub fn to_efivar_data(&self) -> EfiVarData {
        EfiVarData::new(0x7, [*self as u8, 0x0, 0x0, 0x0])
    }

    /// RootFsStatus from EfiVar raw bytes
    pub fn from_efivar_data(data: &EfiVarData) -> Result<RootFsStatus> {
        if data.len() != 8 {
            return Err(Error::InvalidEfiVarLen {
                expected: 8,
                actual: data.len(),
            });
        }

        match data.value() {
            val if val == Self::NORMAL => Ok(Self::Normal),
            val if val == Self::UPDATE_IN_PROGRESS => Ok(Self::UpdateInProcess),
            val if val == Self::UPDATE_DONE => Ok(Self::UpdateDone),
            val if val == Self::UNBOOTABLE => Ok(Self::Unbootable),
            _ => Err(Error::InvalidRootFsStatusData),
        }
    }
}
