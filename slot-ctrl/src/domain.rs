use derive_more::Display;
use efivar::EfiVarData;
use orb_info::orb_os_release::OrbType;

pub type Result<T> = std::result::Result<T, Error>;

/// Error definition for library.
#[allow(missing_docs)]
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("failed reading efivar, invalid data length. expected: {expected}, actual: {actual}")]
    InvalidEfiVarLen { expected: usize, actual: usize },
    #[error("invalid slot configuration")]
    InvalidSlotData,
    #[error("invalid rootfs status")]
    InvalidRootFsStatusData,
    #[error("invalid retry counter({counter}), exceeding the maximum ({max})")]
    ExceedingRetryCount { counter: u8, max: u8 },
    #[error("{0}")]
    EfiVar(#[from] color_eyre::Report),
    #[error("unsupported orb type: {0}")]
    UnsupportedOrbType(OrbType),
    #[error("{0}")]
    Verification(String),
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
    pub fn into_efivar_data(self) -> EfiVarData {
        EfiVarData::new(0x7, [self as u8, 0x0, 0x0, 0x0])
    }

    pub fn from_efivar_data(data: &EfiVarData) -> Result<Self> {
        let len = data.len();
        if len != 8 {
            return Err(Error::InvalidEfiVarLen {
                expected: 8,
                actual: len,
            });
        }

        match data.value()[4] {
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
            _ => Err(Error::InvalidRootFsStatusData),
        }
    }
}

/// Representation of the slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Display)]
pub enum Slot {
    #[display("a")]
    A = 1,
    #[display("b")]
    B = 2,
}

/// Representation of the rootfs status.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum RootFsStatus {
    /// Default status of the rootfs.
    Normal,
    /// Status of the rootfs where the partitions during an update are written.
    UpdateInProcess,
    /// Status of the rootfs where the boot slot was just switched to it.
    UpdateDone,
    /// Status of the rootfs is considered unbootable.
    Unbootable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Display)]
pub struct RetryCount(pub u8);

impl RetryCount {
    pub fn to_efivar_data(&self) -> EfiVarData {
        EfiVarData::new(0x7, [self.0, 0x0, 0x0, 0x0])
    }

    pub fn from_efivar_data(data: &EfiVarData) -> Result<RetryCount> {
        let len = data.len();
        if len != 8 {
            return Err(Error::InvalidEfiVarLen {
                expected: 8,
                actual: len,
            });
        }

        // While the data part of the retry count EFI var is 4 bytes,
        // the retry count is only stored in the first byte.
        Ok(RetryCount(data.value()[0]))
    }
}

impl Slot {
    const SLOT_A_BYTES: [u8; 4] = [0x00, 0x00, 0x00, 0x00];
    const SLOT_B_BYTES: [u8; 4] = [0x01, 0x00, 0x00, 0x00];

    pub const CURRENT_SLOT_PATH: &str =
        "BootChainFwCurrent-781e084c-a330-417c-b678-38e696380cb9";
    pub const NEXT_SLOT_PATH: &str =
        "BootChainFwNext-781e084c-a330-417c-b678-38e696380cb9";
    pub const BOOTCHAIN_STATUS_PATH: &str =
        "BootChainFwStatus-781e084c-a330-417c-b678-38e696380cb9";

    /// Slot as EfiVar raw bytes
    pub fn to_efivar_data(&self) -> EfiVarData {
        match self {
            Slot::A => EfiVarData::new(0x7, Self::SLOT_A_BYTES),
            Slot::B => EfiVarData::new(0x7, Self::SLOT_B_BYTES),
        }
    }

    pub fn from_efivar_data(data: &EfiVarData) -> Result<Slot> {
        if Slot::SLOT_A_BYTES == data.value() {
            Ok(Slot::A)
        } else if Slot::SLOT_B_BYTES == data.value() {
            Ok(Slot::B)
        } else {
            Err(Error::InvalidSlotData)
        }
    }
}

impl RootFsStatus {
    // Right now Pearl has extra states in the update status, some thing
    // we will probably get rid of in the future. Values were also altered and are
    // different than the default NVIDIA ones (used by Diamond)
    // https://github.com/worldcoin/edk2-nvidia/blob/ede09eb66b00d5d185ba93b7992390f2a483b46f/Silicon/NVIDIA/Include/NVIDIAConfiguration.h#L23
    const PEARL_NORMAL: [u8; 4] = [0x00, 0x00, 0x00, 0x00];
    const PEARL_UPDATE_IN_PROGRESS: [u8; 4] = [0x01, 0x00, 0x00, 0x00];
    const PEARL_UPDATE_DONE: [u8; 4] = [0x02, 0x00, 0x00, 0x00];
    const PEARL_UNBOOTABLE: [u8; 4] = [0x03, 0x00, 0x00, 0x00];

    // https://github.com/worldcoin/edk2-nvidia/blob/86a32d95373d6aaf87278093a855ccf193b9c61f/Silicon/NVIDIA/Include/NVIDIAConfiguration.h#L23
    const DIAMOND_NORMAL: [u8; 4] = [0x00, 0x00, 0x00, 0x00];
    const DIAMOND_UNBOOTABLE: [u8; 4] = [0xFF, 0x00, 0x00, 0x00];

    pub const STATUS_A_PATH: &str =
        "RootfsStatusSlotA-781e084c-a330-417c-b678-38e696380cb9";
    pub const STATUS_B_PATH: &str =
        "RootfsStatusSlotB-781e084c-a330-417c-b678-38e696380cb9";
    pub const RETRY_COUNT_A_PATH: &str =
        "RootfsRetryCountA-781e084c-a330-417c-b678-38e696380cb9";
    pub const RETRY_COUNT_B_PATH: &str =
        "RootfsRetryCountB-781e084c-a330-417c-b678-38e696380cb9";
    pub const RETRY_COUNT_MAX_PATH: &str =
        "RootfsRetryCountMax-781e084c-a330-417c-b678-38e696380cb9";

    pub fn to_efivar_data(&self, orb: OrbType) -> Result<EfiVarData> {
        let value = match (self, orb) {
            (Self::Normal, OrbType::Pearl) => &Self::PEARL_NORMAL,
            (Self::UpdateInProcess, OrbType::Pearl) => &Self::PEARL_UPDATE_IN_PROGRESS,
            (Self::UpdateDone, OrbType::Pearl) => &Self::PEARL_UPDATE_DONE,
            (Self::Unbootable, OrbType::Pearl) => &Self::PEARL_UNBOOTABLE,
            (Self::Normal, OrbType::Diamond) => &Self::DIAMOND_NORMAL,
            (Self::Unbootable, OrbType::Diamond) => &Self::DIAMOND_UNBOOTABLE,
            _ => return Err(Error::InvalidRootFsStatusData),
        };

        Ok(EfiVarData::new(0x7, value))
    }

    /// RootFsStatus from EfiVar raw bytes
    pub fn from_efivar_data(data: &EfiVarData, orb: OrbType) -> Result<RootFsStatus> {
        let bytes = data.value();

        match orb {
            OrbType::Pearl if bytes == Self::PEARL_NORMAL => Ok(RootFsStatus::Normal),

            OrbType::Pearl if bytes == Self::PEARL_UPDATE_IN_PROGRESS => {
                Ok(RootFsStatus::UpdateInProcess)
            }

            OrbType::Pearl if bytes == Self::PEARL_UPDATE_DONE => {
                Ok(RootFsStatus::UpdateDone)
            }

            OrbType::Pearl if bytes == Self::PEARL_UNBOOTABLE => {
                Ok(RootFsStatus::Unbootable)
            }

            OrbType::Diamond if bytes == Self::DIAMOND_NORMAL => {
                Ok(RootFsStatus::Normal)
            }

            OrbType::Diamond if bytes == Self::DIAMOND_UNBOOTABLE => {
                Ok(RootFsStatus::Unbootable)
            }

            _ => Err(Error::InvalidRootFsStatusData),
        }
    }
}
